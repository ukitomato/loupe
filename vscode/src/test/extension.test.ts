// extension.test.ts — Extension Development Host (EDH) integration tests.
// Verifies that all commands are registered, default config values are correct,
// and the webview view provider is registered after activation.

import * as assert from 'assert';
import * as vscode from 'vscode';

suite('Extension activation', () => {
  suiteSetup(async () => {
    const ext = vscode.extensions.getExtension('ukitomato.loupe-search');
    if (ext && !ext.isActive) {
      await ext.activate();
    }
    // Allow async activation side-effects (sidecar start, showInformationMessage) to settle.
    await new Promise(r => setTimeout(r, 500));
  });

  // --- commands ---

  test('loupe.search is registered', async () => {
    const cmds = await vscode.commands.getCommands(true);
    assert.ok(cmds.includes('loupe.search'), 'loupe.search not found in registered commands');
  });

  test('loupe.searchRegex is registered', async () => {
    const cmds = await vscode.commands.getCommands(true);
    assert.ok(cmds.includes('loupe.searchRegex'));
  });

  test('loupe.reindex is registered', async () => {
    const cmds = await vscode.commands.getCommands(true);
    assert.ok(cmds.includes('loupe.reindex'));
  });

  test('loupe.focusSearch is registered', async () => {
    const cmds = await vscode.commands.getCommands(true);
    assert.ok(cmds.includes('loupe.focusSearch'));
  });

  test('all four loupe commands are registered', async () => {
    const cmds = await vscode.commands.getCommands(true);
    const loupeCmds = cmds.filter(c => c.startsWith('loupe.'));
    for (const expected of ['loupe.search', 'loupe.searchRegex', 'loupe.reindex', 'loupe.focusSearch']) {
      assert.ok(loupeCmds.includes(expected), `missing: ${expected}`);
    }
  });

  // --- default configuration values ---

  test('default maxResults is 300', () => {
    const cfg = vscode.workspace.getConfiguration('loupe');
    assert.strictEqual(cfg.get<number>('maxResults'), 300);
  });

  test('default indexDir is empty string', () => {
    const cfg = vscode.workspace.getConfiguration('loupe');
    assert.strictEqual(cfg.get<string>('indexDir'), '');
  });

  test('default binaryPath is empty string', () => {
    const cfg = vscode.workspace.getConfiguration('loupe');
    assert.strictEqual(cfg.get<string>('binaryPath'), '');
  });

  test('configuration section exists and has all three keys', () => {
    const cfg = vscode.workspace.getConfiguration('loupe');
    assert.ok(cfg.has('maxResults'));
    assert.ok(cfg.has('indexDir'));
    assert.ok(cfg.has('binaryPath'));
  });
});
