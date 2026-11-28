#!/usr/bin/env node
/**
 * Reproducible 1920x1080 @ 60fps Termexo Fleet promotional launch video renderer.
 *
 * Pipeline:
 *   1. Serves the live production Angular build (apps/desktop-ui/dist/desktop-ui/browser)
 *      and captures 10 fresh 2x Retina UI screens via headless Edge + Playwright.
 *   2. Synthesizes a frame-accurate 48 kHz stereo soundtrack + UI SFX (promo-audio.wav).
 *   3. Renders 2,700 deterministic frames (45.0s @ 60fps) from scripts/promo/stage.html
 *      via Chrome DevTools Protocol piped straight into FFmpeg (H.264 High, CRF 17, AAC 256k).
 *   4. Writes docs/promo/termexo-fleet-promo.mp4 and docs/promo/termexo-fleet-promo-cover.png.
 *
 * Usage:
 *   node scripts/render-promo-video.mjs              # 1920x1080 landscape (default)
 *   node scripts/render-promo-video.mjs --vertical   # 1080x1920 9:16 mobile portrait
 */
import { spawn, spawnSync } from 'node:child_process';
import { copyFileSync, createReadStream, existsSync, mkdirSync, rmSync, statSync, writeFileSync } from 'node:fs';
import http from 'node:http';
import { createRequire } from 'node:module';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

import { generateSoundtrackWav } from './promo/soundtrack.mjs';

const require = createRequire(import.meta.url);
const __dirname = path.dirname(fileURLToPath(import.meta.url));
const repoRoot = path.resolve(__dirname, '..');

const distDir = path.join(repoRoot, 'apps', 'desktop-ui', 'dist', 'desktop-ui', 'browser');
const outDir = path.join(repoRoot, 'docs', 'promo');
const tmpDir = path.join(os.tmpdir(), `termexo-fleet-promo-${Date.now()}`);
const stageAssetsDir = path.join(tmpDir, 'assets');

const FPS = Number(process.env.PROMO_FPS || 60);
const DURATION_SEC = 45.0;
const TOTAL_FRAMES = Math.round(DURATION_SEC * FPS);
const isVertical = process.argv.includes('--vertical');
const STAGE_WIDTH = isVertical ? 1080 : 1920;
const STAGE_HEIGHT = isVertical ? 1920 : 1080;
const stageSourceFile = isVertical ? 'stage-vertical.html' : 'stage.html';
const outBaseName = isVertical ? 'termexo-fleet-promo-vertical' : 'termexo-fleet-promo';
const previewFlagIdx = process.argv.indexOf('--preview-frames');
const previewFramesDir = previewFlagIdx >= 0 ? process.argv[previewFlagIdx + 1] : null;

if (!existsSync(path.join(distDir, 'index.html'))) {
  console.error(`Production UI build not found at ${distDir}. Run "npm run build" first.`);
  process.exit(1);
}

function loadPlaywright() {
  const candidates = [
    'playwright',
    path.join(process.env.APPDATA || '', 'npm', 'node_modules', 'playwright'),
    '/usr/local/lib/node_modules/playwright',
  ];
  for (const mod of candidates) {
    if (!mod) continue;
    try {
      return require(mod);
    } catch {
      // try next
    }
  }
  throw new Error('Playwright is required to render the promo video (npm install -g playwright).');
}

function findBrowserExecutable() {
  const candidates = [
    process.env.PROMO_BROWSER_PATH,
    'C:\\Program Files (x86)\\Microsoft\\Edge\\Application\\msedge.exe',
    'C:\\Program Files\\Microsoft\\Edge\\Application\\msedge.exe',
    'C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe',
    '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome',
    '/usr/bin/chromium',
    '/usr/bin/google-chrome',
  ];
  return candidates.find((p) => p && existsSync(p));
}

const MIME = {
  '.html': 'text/html; charset=utf-8',
  '.js': 'text/javascript; charset=utf-8',
  '.css': 'text/css; charset=utf-8',
  '.svg': 'image/svg+xml',
  '.png': 'image/png',
  '.ico': 'image/x-icon',
  '.json': 'application/json',
  '.woff2': 'font/woff2',
};

function startStaticServer(rootDir) {
  return new Promise((resolve) => {
    const server = http.createServer((req, res) => {
      const clean = decodeURIComponent((req.url || '/').split('?')[0]);
      let fp = path.join(rootDir, clean === '/' ? 'index.html' : clean);
      if (!existsSync(fp) || statSync(fp).isDirectory()) {
        const fallback = path.join(rootDir, 'index.html');
        if (existsSync(fallback)) {
          fp = fallback;
        } else {
          res.writeHead(404);
          res.end();
          return;
        }
      }
      const ext = path.extname(fp).toLowerCase();
      res.writeHead(200, { 'Content-Type': MIME[ext] || 'application/octet-stream' });
      createReadStream(fp).pipe(res);
    });
    server.listen(0, '127.0.0.1', () => {
      const { port } = server.address();
      resolve({ server, url: `http://127.0.0.1:${port}/` });
    });
  });
}

mkdirSync(outDir, { recursive: true });
mkdirSync(stageAssetsDir, { recursive: true });
copyFileSync(path.join(__dirname, 'promo', stageSourceFile), path.join(tmpDir, 'stage.html'));
copyFileSync(path.join(repoRoot, 'apps', 'desktop-ui', 'public', 'fleet-mark.svg'), path.join(stageAssetsDir, 'fleet-mark.svg'));

const { chromium } = loadPlaywright();
const executablePath = findBrowserExecutable();
const browser = await chromium.launch({
  ...(executablePath ? { executablePath } : {}),
  headless: true,
  args: ['--disable-gpu-vsync', '--font-render-hinting=none'],
});

const uiServer = await startStaticServer(distDir);
const stageServer = await startStaticServer(tmpDir);

try {
  // ==================== 1. CAPTURE HIGH-DPI LIVE UI ASSETS ====================
  const captureDpr = isVertical ? 3 : 2;
  console.log(`[1/4] Capturing ${captureDpr}x Retina live Termexo Fleet UI screens...`);
  const uiPage = await browser.newPage({
    viewport: { width: 1440, height: 900 },
    deviceScaleFactor: captureDpr,
  });
  await uiPage.goto(uiServer.url, { waitUntil: 'networkidle' });
  await uiPage.waitForTimeout(450);

  // Polish browser-preview footer text purely for the launch film so it matches native Tauri desktop
  await uiPage.evaluate(() => {
    const walker = document.createTreeWalker(document.body, NodeFilter.SHOW_TEXT);
    while (walker.nextNode()) {
      const node = walker.currentNode;
      if (node.nodeValue && node.nodeValue.includes('浏览器预览模式')) {
        node.nodeValue = node.nodeValue.replace('浏览器预览模式', '本地引擎在线 · Rust Core');
      }
    }
  });

  const rectOf = async (locator) => {
    const box = await locator.first().boundingBox();
    return box ? { x: Math.round(box.x), y: Math.round(box.y), w: Math.round(box.width), h: Math.round(box.height) } : null;
  };
  const coords = {};

  // Clean mission deck
  coords.goalInput = await rectOf(uiPage.locator('.prompt-goal-input'));
  coords.forecast = await rectOf(uiPage.locator('.route-forecast-ribbon'));
  coords.dag = await rectOf(uiPage.locator('.dag-pipeline'));
  coords.termBtn = await rectOf(uiPage.locator('.terminal-action-btn'));
  await uiPage.screenshot({ path: path.join(stageAssetsDir, 'missions-clean.png') });

  // Typed preset mission deck (L3/L4 route preview glowing)
  await uiPage.locator('.preset-chip', { hasText: '架构重构 + L4 审查' }).click();
  await uiPage.waitForTimeout(220);
  await uiPage.screenshot({ path: path.join(stageAssetsDir, 'missions-typed.png') });

  // Slide-over Live Terminal Drawer
  await uiPage.locator('.terminal-action-btn').first().click();
  await uiPage.waitForTimeout(320);
  coords.termViewport = await rectOf(uiPage.locator('.terminal-output'));
  coords.drawerReply = await rectOf(uiPage.locator('.drawer-reply-bar'));
  coords.approveBtn = await rectOf(uiPage.locator('.drawer-reply-bar .quick-chip', { hasText: '同意' }));
  await uiPage.screenshot({ path: path.join(stageAssetsDir, 'terminal-drawer.png') });
  await uiPage.keyboard.press('Escape');
  await uiPage.waitForTimeout(180);

  // Routing Ladder: L1, L3, L4 dynamic states
  await uiPage.locator('.nav-item', { hasText: '难度天梯与账号池' }).click();
  await uiPage.waitForTimeout(220);
  const chipL1 = uiPage.locator('.sim-sample-chip', { hasText: '修文档错别字' });
  const chipL3 = uiPage.locator('.sim-sample-chip', { hasText: '重构并发调度' });
  const chipL4 = uiPage.locator('.sim-sample-chip', { hasText: '安全交叉审查' });
  coords.chipL1 = await rectOf(chipL1);
  coords.chipL3 = await rectOf(chipL3);
  coords.chipL4 = await rectOf(chipL4);

  await chipL1.click();
  await uiPage.waitForTimeout(160);
  await uiPage.screenshot({ path: path.join(stageAssetsDir, 'routing-l1.png') });

  await chipL3.click();
  await uiPage.waitForTimeout(160);
  await uiPage.screenshot({ path: path.join(stageAssetsDir, 'routing-l3.png') });

  await chipL4.click();
  await uiPage.waitForTimeout(160);
  await uiPage.screenshot({ path: path.join(stageAssetsDir, 'routing-l4.png') });

  // Sub-Agent Swarm Tree
  await uiPage.locator('.nav-item', { hasText: '子 Agent 蜂群树' }).click();
  await uiPage.waitForTimeout(220);
  coords.swarmTree = await rectOf(uiPage.locator('.swarm-tree'));
  await uiPage.screenshot({ path: path.join(stageAssetsDir, 'swarm-tree.png') });

  // Multi-Machine Fleet Topology
  await uiPage.locator('.nav-item', { hasText: '多机舰队拓扑' }).click();
  await uiPage.waitForTimeout(220);
  coords.topology = await rectOf(uiPage.locator('.topology-canvas'));
  await uiPage.screenshot({ path: path.join(stageAssetsDir, 'topology.png') });

  // Execution Ledger & Multi-Channel Alerts (enable switches so emerald UI shines)
  await uiPage.locator('.nav-item', { hasText: '效能账本与通知' }).click();
  await uiPage.waitForTimeout(220);
  const switches = uiPage.locator('.notify-channel .ui-switch');
  if ((await switches.count()) >= 2) {
    await switches.nth(0).click();
    await switches.nth(1).click();
    await uiPage.waitForTimeout(160);
  }
  await uiPage.screenshot({ path: path.join(stageAssetsDir, 'analytics.png') });

  // Global Command Palette (Ctrl+K)
  await uiPage.keyboard.press('Control+k');
  await uiPage.waitForTimeout(260);
  await uiPage.screenshot({ path: path.join(stageAssetsDir, 'palette.png') });
  writeFileSync(path.join(stageAssetsDir, 'layout-coords.json'), JSON.stringify(coords, null, 2));
  await uiPage.close();

  // ==================== 2. SYNTHESIZE STUDIO SOUNDTRACK & SFX ====================
  console.log('[2/4] Synthesizing 48 kHz stereo soundtrack & synced SFX...');
  const audioPath = path.join(tmpDir, 'promo-audio.wav');
  const audioInfo = generateSoundtrackWav(audioPath);
  console.log(`      Audio ready: ${audioInfo.duration}s stereo WAV (${(audioInfo.bytes / 1048576).toFixed(2)} MiB)`);

  // ==================== 3. RENDER 60FPS STREAM TO FFMPEG ====================
  const outMp4 = path.join(outDir, `${outBaseName}.mp4`);
  const outCover = path.join(outDir, `${outBaseName}-cover.png`);
  console.log(`[3/4] Rendering ${TOTAL_FRAMES} frames (${DURATION_SEC}s @ ${FPS}fps, ${STAGE_WIDTH}x${STAGE_HEIGHT})...`);

  const stagePage = await browser.newPage({
    viewport: { width: STAGE_WIDTH, height: STAGE_HEIGHT },
    deviceScaleFactor: 1,
  });
  await stagePage.goto(`${stageServer.url}stage.html`, { waitUntil: 'networkidle' });
  await stagePage.evaluate(async () => {
    await document.fonts.ready;
    if (window.__coordsReady) await window.__coordsReady;
  });
  await stagePage.waitForTimeout(250);

  if (previewFramesDir) {
    mkdirSync(previewFramesDir, { recursive: true });
    const stamps = [
      ['01-hook', 3.4],
      ['02-hero', 6.8],
      ['03-typing', 12.6],
      ['04-forecast', 13.8],
      ['05-dag', 16.9],
      ['06-terminal', 20.6],
      ['07-routing-l1', 23.6],
      ['08-routing-l4', 27.1],
      ['09-swarm', 29.5],
      ['10-topology', 32.8],
      ['11-analytics', 36.2],
      ['11b-palette', 38.1],
      ['12-outro', 42.4],
    ];
    for (const [label, sec] of stamps) {
      await stagePage.evaluate((s) => window.__setTime(s), sec);
      await stagePage.screenshot({ path: path.join(previewFramesDir, `${label}.png`) });
    }
    await stagePage.evaluate(() => window.__renderCover());
    await stagePage.screenshot({ path: path.join(previewFramesDir, '00-cover.png') });
    console.log(`PREVIEW_FRAMES_OK (${stamps.length + 1} keyframes -> ${previewFramesDir})`);
  } else {
  const cdp = await stagePage.context().newCDPSession(stagePage);

  const ffmpeg = spawn(
    'ffmpeg',
    [
      '-hide_banner',
      '-loglevel',
      'error',
      '-y',
      '-f',
      'image2pipe',
      '-vcodec',
      'png',
      '-framerate',
      String(FPS),
      '-i',
      'pipe:0',
      '-i',
      audioPath,
      '-map',
      '0:v:0',
      '-map',
      '1:a:0',
      '-c:v',
      'libx264',
      '-preset',
      'slow',
      '-crf',
      '17',
      '-profile:v',
      'high',
      '-pix_fmt',
      'yuv420p',
      '-c:a',
      'aac',
      '-b:a',
      '256k',
      '-movflags',
      '+faststart',
      '-shortest',
      outMp4,
    ],
    { stdio: ['pipe', 'inherit', 'inherit'] },
  );

  const ffmpegDone = new Promise((resolve, reject) => {
    ffmpeg.on('error', reject);
    ffmpeg.on('close', (code) => {
      if (code === 0) resolve();
      else reject(new Error(`ffmpeg exited with code ${code}`));
    });
  });

  const tStart = Date.now();
  for (let frame = 0; frame < TOTAL_FRAMES; frame++) {
    const t = frame / FPS;
    await stagePage.evaluate((sec) => window.__setTime(sec), t);
    const { data } = await cdp.send('Page.captureScreenshot', {
      format: 'png',
      fromSurface: true,
      optimizeForSpeed: true,
    });
    const pngBuf = Buffer.from(data, 'base64');
    if (!ffmpeg.stdin.write(pngBuf)) {
      await new Promise((r) => ffmpeg.stdin.once('drain', r));
    }
    if ((frame + 1) % (FPS * 5) === 0 || frame + 1 === TOTAL_FRAMES) {
      const elapsed = ((Date.now() - tStart) / 1000).toFixed(1);
      const pct = Math.round(((frame + 1) / TOTAL_FRAMES) * 100);
      console.log(`      ${pct}% (${frame + 1}/${TOTAL_FRAMES} frames · ${elapsed}s elapsed)`);
    }
  }
  ffmpeg.stdin.end();
  await ffmpegDone;

  // ==================== 4. EXTRACT HERO POSTER & PROBE OUTPUT ====================
  console.log('[4/4] Generating crisp 1920x1080 hero cover poster & verifying stream...');
  await stagePage.evaluate(() => window.__renderCover());
  await stagePage.screenshot({ path: outCover });

  const probeRes = spawnSync(
    'ffprobe',
    [
      '-v',
      'error',
      '-show_entries',
      'format=duration,size:stream=codec_type,codec_name,width,height,r_frame_rate,sample_rate,channels',
      '-of',
      'json',
      outMp4,
    ],
    { encoding: 'utf8' },
  );
  if (probeRes.status !== 0) {
    throw new Error(`ffprobe failed: ${probeRes.stderr}`);
  }
  const probe = JSON.parse(probeRes.stdout);
  console.log(
    JSON.stringify(
      {
        video: outMp4,
        cover: outCover,
        sizeMiB: Number((Number(probe.format.size) / 1048576).toFixed(2)),
        durationSec: Number(Number(probe.format.duration).toFixed(2)),
        streams: probe.streams,
      },
      null,
      2,
    ),
  );
  }
} finally {
  await browser.close();
  uiServer.server.close();
  stageServer.server.close();
  rmSync(tmpDir, { recursive: true, force: true });
}
