#!/usr/bin/env node
/** Capture README screenshots from static HTML mocks (uses the real searchView.css). */
import { chromium } from 'playwright';
import { fileURLToPath } from 'node:url';
import path from 'node:path';

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const mocksDir = path.join(__dirname, 'readme-screenshots');
const outDir = path.join(__dirname, '..', 'images');

const shots = [
  { html: 'sidebar.html', out: 'sidebar-search.png', selector: 'body', width: 380, height: 520 },
  { html: 'quickpick.html', out: 'quickpick-search.png', selector: '.quickpick', width: 560, height: 360 },
];

const browser = await chromium.launch();
const page = await browser.newPage({ deviceScaleFactor: 2 });

for (const shot of shots) {
  const file = path.join(mocksDir, shot.html);
  await page.setViewportSize({ width: shot.width, height: shot.height });
  await page.goto(`file://${file}`);
  const el = await page.locator(shot.selector);
  await el.screenshot({ path: path.join(outDir, shot.out) });
  console.log(`wrote ${path.join(outDir, shot.out)}`);
}

await browser.close();
