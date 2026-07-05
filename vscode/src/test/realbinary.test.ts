// realbinary.test.ts — End-to-end tests against the real loupe binary.
// Skipped automatically when the binary is not found; no manual configuration needed.
// In CI the rust job builds the binary and places it at vscode/bin/<os>-<arch>/loupe.

import * as assert from 'assert';
import * as os from 'os';
import * as path from 'path';
import * as fs from 'fs';
import * as cp from 'child_process';
import { Sidecar } from '../sidecarClient';

const EXE = process.platform === 'win32' ? 'loupe.exe' : 'loupe';
const ARCH = process.arch === 'arm64' ? 'arm64' : 'x64';
const OS_DIR = process.platform === 'win32' ? 'win32' : process.platform === 'darwin' ? 'darwin' : 'linux';
const PLATFORM = `${OS_DIR}-${ARCH}`;

/** Locate the loupe binary. Returns undefined if not found. */
function findBinary(): string | undefined {
  // Possible locations relative to this compiled test file (out/test/realbinary.test.js):
  //   - CI layout:   out/test/ -> vscode/bin/<platform>/
  //   - Repo layout: out/test/ -> tools/loupe/bin/<platform>/
  const base = path.join(__dirname, '..', '..'); // vscode/
  const candidates = [
    path.join(base, 'bin', PLATFORM, EXE),
    path.join(base, '..', 'bin', PLATFORM, EXE),  // tools/loupe/bin/
  ];
  for (const p of candidates) {
    if (fs.existsSync(p)) return p;
  }

  // Fallback: try PATH
  try {
    const r = cp.spawnSync(process.platform === 'win32' ? 'where' : 'which', ['loupe'], { encoding: 'utf8', timeout: 3000 });
    if (r.status === 0 && r.stdout.trim()) {
      return r.stdout.trim().split('\n')[0].trim();
    }
  } catch { /* not on PATH */ }

  return undefined;
}

// Use function() (not arrow) so Mocha's `this` context is available for this.timeout() etc.
suite('Real binary e2e', function () {
  this.timeout(30_000);

  let binaryPath: string;
  let tmpDir: string;

  suiteSetup(function (this: Mocha.Context) {
    const bin = findBinary();
    if (!bin) {
      console.log(`[skip] loupe binary not found (looked in vscode/bin/${PLATFORM}/ and PATH)`);
      this.skip();
      return;
    }
    binaryPath = bin;
    tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), 'loupe-vscode-test-'));
  });

  suiteTeardown(() => {
    if (tmpDir) {
      try { fs.rmSync(tmpDir, { recursive: true, force: true }); } catch { /* ignore */ }
    }
  });

  // --- sidecar startup ---

  test('sidecar emits {"type":"ready"} on startup', async () => {
    const idxDir = path.join(tmpDir, 'ready-test', '.loupe');
    fs.mkdirSync(idxDir, { recursive: true });
    const s = new Sidecar(binaryPath, idxDir);
    try {
      s.start();
      // ready is set in start(); await it with a fallback timeout
      await Promise.race([
        (s as any).ready as Promise<void>,
        new Promise<void>((_, rej) => setTimeout(() => rej(new Error('ready timeout (5s)')), 5_000)),
      ]);
    } finally {
      s.dispose();
    }
  });

  // --- init + build + search flow ---

  test('init + build via CLI, then search via sidecar returns a hit', async () => {
    const root = path.join(tmpDir, 'src');
    fs.mkdirSync(root, { recursive: true });
    fs.writeFileSync(path.join(root, 'hello.txt'), 'helloworld search_target\n');

    const idxDir = path.join(tmpDir, '.loupe');

    // init
    const initR = cp.spawnSync(binaryPath, ['init', '--root', root, '--index-dir', idxDir], { encoding: 'utf8' });
    assert.strictEqual(initR.status, 0, `init failed:\n${initR.stderr}`);

    // build
    const buildR = cp.spawnSync(binaryPath, ['build', '--index-dir', idxDir], { encoding: 'utf8' });
    assert.strictEqual(buildR.status, 0, `build failed:\n${buildR.stderr}`);

    // sidecar search
    const s = new Sidecar(binaryPath, idxDir);
    try {
      s.start();
      await Promise.race([
        (s as any).ready as Promise<void>,
        new Promise<void>((_, rej) => setTimeout(() => rej(new Error('ready timeout')), 5_000)),
      ]);

      const hits: Array<{ file: string; line: number; text: string }> = [];
      const result = await s.search('helloworld', false, 10, false, m => hits.push(m));

      assert.ok(result.hits > 0, `Expected ≥1 hit, got ${result.hits}`);
      assert.ok(hits.length > 0, 'onMatch should have been called at least once');
      assert.ok(hits[0].file.endsWith('hello.txt'), `Expected hit in hello.txt, got ${hits[0].file}`);
      assert.strictEqual(hits[0].line, 1);
    } finally {
      s.dispose();
    }
  });

  test('search for a 2-char query returns 0 hits (query too short for index)', async () => {
    const idxDir = path.join(tmpDir, '.loupe');

    const s = new Sidecar(binaryPath, idxDir);
    try {
      s.start();
      await Promise.race([
        (s as any).ready as Promise<void>,
        new Promise<void>((_, rej) => setTimeout(() => rej(new Error('ready timeout')), 5_000)),
      ]);

      const hits: unknown[] = [];
      const result = await s.search('hi', false, 10, false, m => hits.push(m));
      assert.strictEqual(result.hits, 0);
      assert.strictEqual(hits.length, 0);
    } finally {
      s.dispose();
    }
  });

  test('sidecar sync after no changes reports 0 updated', async () => {
    const idxDir = path.join(tmpDir, '.loupe');

    const s = new Sidecar(binaryPath, idxDir);
    try {
      s.start();
      await Promise.race([
        (s as any).ready as Promise<void>,
        new Promise<void>((_, rej) => setTimeout(() => rej(new Error('ready timeout')), 5_000)),
      ]);

      const result = await s.sync(() => {});
      assert.strictEqual(result.updated, 0, 'nothing changed so updated should be 0');
    } finally {
      s.dispose();
    }
  });
});
