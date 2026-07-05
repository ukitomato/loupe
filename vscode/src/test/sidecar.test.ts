// sidecar.test.ts — NDJSON protocol tests for Sidecar class.
// No real binary is spawned: `send()` is mocked and `onLine()` is called directly
// to simulate protocol messages. Tests focus on id correlation, streaming callbacks,
// promise resolution/rejection, and edge cases.

import * as assert from 'assert';
import { Sidecar, type Match } from '../sidecarClient';

/** Create a Sidecar with no real process: ready resolves immediately, send is a no-op. */
function makeMock(): Sidecar {
  const s = new Sidecar('', '');
  (s as any).ready = Promise.resolve();
  (s as any).send = () => { /* no-op: no real stdin to write to */ };
  return s;
}

/** Simulate an incoming NDJSON line from the sidecar process. */
function sim(s: Sidecar, msg: object): void {
  (s as any).onLine(JSON.stringify(msg));
}

/** search/build/sync/watch are async; yield so pending registration runs before sim(). */
async function afterRequest(): Promise<void> {
  await Promise.resolve();
}

// --- ready handshake ---

suite('Sidecar: ready handshake', () => {
  test('{"type":"ready"} resolves the ready promise', async () => {
    const s = new Sidecar('', '');
    (s as any).send = () => {};
    let readyResolved = false;
    const waitReady = (s as any).ready === undefined
      ? Promise.resolve()  // start() not called, ready is undefined → await undefined resolves
      : (s as any).ready;

    // Manually initialize the ready promise (mimics start())
    (s as any).ready = new Promise<void>(r => { (s as any).readyResolve = r; });
    const p = (s as any).ready.then(() => { readyResolved = true; });
    assert.strictEqual(readyResolved, false);
    sim(s, { type: 'ready' });
    await p;
    assert.strictEqual(readyResolved, true);
  });
});

// --- search ---

suite('Sidecar: search protocol', () => {
  test('match messages invoke onMatch callback', async () => {
    const s = makeMock();
    const hits: Match[] = [];
    const p = s.search('foo', false, 100, false, m => hits.push(m));
    await afterRequest();
    sim(s, { type: 'match', id: 1, file: '/src/a.ts', line: 3, text: 'foobar' });
    sim(s, { type: 'match', id: 1, file: '/src/b.ts', line: 7, text: 'foo_baz' });
    sim(s, { type: 'done', id: 1, hits: 2 });
    const result = await p;
    assert.strictEqual(result.hits, 2);
    assert.strictEqual(hits.length, 2);
    assert.strictEqual(hits[0].file, '/src/a.ts');
    assert.strictEqual(hits[0].line, 3);
    assert.strictEqual(hits[0].text, 'foobar');
    assert.strictEqual(hits[1].file, '/src/b.ts');
  });

  test('done with zero hits resolves with hits=0 and no onMatch calls', async () => {
    const s = makeMock();
    let called = false;
    const p = s.search('xyz', false, 100, false, () => { called = true; });
    await afterRequest();
    sim(s, { type: 'done', id: 1, hits: 0 });
    const result = await p;
    assert.strictEqual(result.hits, 0);
    assert.strictEqual(called, false);
  });

  test('promise is removed from pending after done', async () => {
    const s = makeMock();
    const p = s.search('foo', false, 100, false, () => {});
    await afterRequest();
    assert.strictEqual((s as any).pending.size, 1);
    sim(s, { type: 'done', id: 1, hits: 0 });
    await p;
    assert.strictEqual((s as any).pending.size, 0);
  });

  test('error with id rejects the corresponding promise', async () => {
    const s = makeMock();
    const p = s.search('foo', false, 100, false, () => {});
    await afterRequest();
    sim(s, { type: 'error', id: 1, message: 'regex too short' });
    await assert.rejects(p, /regex too short/);
    assert.strictEqual((s as any).pending.size, 0);
  });

  test('case_sensitive flag is passed correctly via nextId tracking', async () => {
    const s = makeMock();
    const sent: any[] = [];
    (s as any).send = (obj: any) => sent.push(obj);

    const p1 = s.search('Hello', false, 50, false, () => {});
    const p2 = s.search('World', false, 50, true, () => {});
    await afterRequest();
    sim(s, { type: 'done', id: 1, hits: 0 });
    sim(s, { type: 'done', id: 2, hits: 0 });
    await Promise.all([p1, p2]);

    assert.strictEqual(sent[0].caseSensitive, false);
    assert.strictEqual(sent[1].caseSensitive, true);
    assert.strictEqual(sent[0].query, 'Hello');
    assert.strictEqual(sent[1].query, 'World');
  });
});

// --- build ---

suite('Sidecar: build protocol', () => {
  test('progress messages invoke onProgress callback', async () => {
    const s = makeMock();
    const vals: number[] = [];
    const p = s.build(n => vals.push(n));
    await afterRequest();
    sim(s, { type: 'progress', id: 1, indexed: 100 });
    sim(s, { type: 'progress', id: 1, indexed: 200 });
    sim(s, { type: 'built', id: 1, files: 200, ms: 550, attempts: 1 });
    const result = await p;
    assert.strictEqual(result.files, 200);
    assert.strictEqual(result.ms, 550);
    assert.strictEqual(result.attempts, 1);
    assert.deepStrictEqual(vals, [100, 200]);
  });

  test('progress message with message string passes it to callback', async () => {
    const s = makeMock();
    const msgs: (string | undefined)[] = [];
    const p = s.build((_, msg) => msgs.push(msg));
    await afterRequest();
    sim(s, { type: 'progress', id: 1, indexed: 50, message: 'indexing src/' });
    sim(s, { type: 'built', id: 1, files: 50, ms: 200, attempts: 1 });
    await p;
    assert.deepStrictEqual(msgs, ['indexing src/']);
  });
});

// --- sync ---

suite('Sidecar: sync protocol', () => {
  test('synced resolves with updated/removed/ms', async () => {
    const s = makeMock();
    const vals: number[] = [];
    const p = s.sync(n => vals.push(n));
    await afterRequest();
    sim(s, { type: 'progress', id: 1, indexed: 5 });
    sim(s, { type: 'synced', id: 1, updated: 5, removed: 2, ms: 100 });
    const result = await p;
    assert.strictEqual(result.updated, 5);
    assert.strictEqual(result.removed, 2);
    assert.strictEqual(result.ms, 100);
    assert.deepStrictEqual(vals, [5]);
  });

  test('synced with zero updates/removes', async () => {
    const s = makeMock();
    const p = s.sync(() => {});
    await afterRequest();
    sim(s, { type: 'synced', id: 1, updated: 0, removed: 0, ms: 10 });
    const result = await p;
    assert.strictEqual(result.updated, 0);
    assert.strictEqual(result.removed, 0);
  });
});

// --- watch ---

suite('Sidecar: watch protocol', () => {
  test('watching message resolves the watch promise', async () => {
    const s = makeMock();
    const p = s.watch();
    await afterRequest();
    sim(s, { type: 'watching', id: 1 });
    await p;  // should not throw
  });
});

// --- error handling ---

suite('Sidecar: error handling', () => {
  test('error without id (top-level) rejects all pending requests', async () => {
    const s = makeMock();
    const p1 = s.search('foo', false, 100, false, () => {});
    const p2 = s.build(() => {});
    await afterRequest();
    sim(s, { type: 'error', message: 'fatal index error' });
    const [r1, r2] = await Promise.allSettled([p1, p2]);
    assert.strictEqual(r1.status, 'rejected');
    assert.strictEqual(r2.status, 'rejected');
    assert.ok((r1 as PromiseRejectedResult).reason.message.includes('fatal index error'));
    assert.ok((r2 as PromiseRejectedResult).reason.message.includes('fatal index error'));
  });

  test('error without id clears the pending map', async () => {
    const s = makeMock();
    void s.search('foo', false, 100, false, () => {}).catch(() => {});
    await afterRequest();
    assert.strictEqual((s as any).pending.size, 1);
    sim(s, { type: 'error' });
    // give microtasks a tick
    await Promise.resolve();
    assert.strictEqual((s as any).pending.size, 0);
  });

  test('error with null id is treated as top-level error', async () => {
    const s = makeMock();
    const p = s.search('foo', false, 100, false, () => {});
    await afterRequest();
    sim(s, { type: 'error', id: null, message: 'null id error' });
    const [r] = await Promise.allSettled([p]);
    assert.strictEqual(r.status, 'rejected');
  });

  test('malformed JSON lines are silently ignored', () => {
    const s = makeMock();
    (s as any).onLine('not json');
    (s as any).onLine('{broken:');
    (s as any).onLine('');
    (s as any).onLine('   ');
    // No exception means pass
  });

  test('unknown message type is silently ignored', async () => {
    const s = makeMock();
    const p = s.search('foo', false, 100, false, () => {});
    await afterRequest();
    sim(s, { type: 'unknown_type', id: 1, data: 'xyz' });
    sim(s, { type: 'done', id: 1, hits: 0 });
    await p;  // should still resolve normally
  });

  test('message for unknown id is silently ignored', () => {
    const s = makeMock();
    // No pending requests registered for id 999
    sim(s, { type: 'match', id: 999, file: '/x.ts', line: 1, text: 'x' });
    sim(s, { type: 'done', id: 999, hits: 1 });
    // No exception means pass
  });
});

// --- id correlation ---

suite('Sidecar: id correlation', () => {
  test('two concurrent searches receive only their own matches', async () => {
    const s = makeMock();
    const h1: string[] = [];
    const h2: string[] = [];

    const p1 = s.search('aaa', false, 100, false, m => h1.push(m.file));
    const p2 = s.search('bbb', false, 100, false, m => h2.push(m.file));
    await afterRequest();

    // Interleave: id=2 arrives first
    sim(s, { type: 'match', id: 2, file: '/b1.ts', line: 1, text: 'bbb' });
    sim(s, { type: 'match', id: 1, file: '/a1.ts', line: 1, text: 'aaa' });
    sim(s, { type: 'match', id: 2, file: '/b2.ts', line: 2, text: 'bbb2' });
    sim(s, { type: 'done', id: 1, hits: 1 });
    sim(s, { type: 'done', id: 2, hits: 2 });

    await Promise.all([p1, p2]);
    assert.deepStrictEqual(h1, ['/a1.ts']);
    assert.deepStrictEqual(h2, ['/b1.ts', '/b2.ts']);
  });

  test('three concurrent requests with interleaved progress and done', async () => {
    const s = makeMock();
    const progress: number[] = [];

    const pSearch = s.search('foo', false, 100, false, () => {});
    const pBuild  = s.build(n => progress.push(n));
    const pSync   = s.sync(() => {});
    await afterRequest();

    sim(s, { type: 'progress', id: 2, indexed: 10 });
    sim(s, { type: 'match', id: 1, file: '/f.ts', line: 1, text: 'foo' });
    sim(s, { type: 'done', id: 1, hits: 1 });
    sim(s, { type: 'progress', id: 2, indexed: 20 });
    sim(s, { type: 'synced', id: 3, updated: 0, removed: 0, ms: 5 });
    sim(s, { type: 'built', id: 2, files: 20, ms: 300, attempts: 1 });

    const [rSearch, rBuild, rSync] = await Promise.all([pSearch, pBuild, pSync]);
    assert.strictEqual(rSearch.hits, 1);
    assert.strictEqual(rBuild.files, 20);
    assert.strictEqual(rSync.updated, 0);
    assert.deepStrictEqual(progress, [10, 20]);
  });

  test('nextId increments monotonically per request', async () => {
    const s = makeMock();
    const sent: number[] = [];
    (s as any).send = (obj: any) => sent.push(obj.id);

    void s.search('a', false, 10, false, () => {}).catch(() => {});
    void s.search('b', false, 10, false, () => {}).catch(() => {});
    void s.build(() => {}).catch(() => {});
    await afterRequest();

    assert.deepStrictEqual(sent, [1, 2, 3]);
  });
});

// --- dispose ---

suite('Sidecar: dispose', () => {
  test('dispose sends stop command', () => {
    const s = makeMock();
    const sent: any[] = [];
    (s as any).send = (obj: any) => sent.push(obj);
    (s as any).started = true;  // pretend started so dispose sends stop
    s.dispose();
    assert.ok(sent.some(m => m.cmd === 'stop'), 'dispose should send {"cmd":"stop"}');
  });

  test('dispose is idempotent (can be called multiple times)', () => {
    const s = makeMock();
    (s as any).started = true;
    s.dispose();
    s.dispose();  // should not throw
  });
});
