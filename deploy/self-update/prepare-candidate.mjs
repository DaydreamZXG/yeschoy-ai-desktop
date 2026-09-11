import { execFileSync } from 'node:child_process';
import { readFileSync, mkdirSync, copyFileSync, existsSync, mkdtempSync, writeFileSync, readdirSync, statSync } from 'node:fs';
import { resolve } from 'node:path';
import { createHash } from 'node:crypto';
import assert from 'node:assert/strict';

const repo = resolve(import.meta.dirname, '../..');
const output = resolve(process.argv[2]);
assert(output.startsWith(`${repo}/release/internal/`));
const version = JSON.parse(readFileSync(`${repo}/package.json`)).version;
const candidate = `${output}/candidate`;
assert(!existsSync(candidate), 'Use a fresh candidate directory');
mkdirSync(candidate, { mode: 0o700 });
const hash = path => createHash('sha256').update(readFileSync(path)).digest('hex');
const receipts = [];
for (const variant of ['official', 'partner']) {
  for (const platform of ['windows', 'macos']) {
    const source = JSON.parse(readFileSync(`${output}/${platform}/evidence/${variant}-${platform}-source.json`));
    for (const [path, digest] of Object.entries(source.pathHashes)) {
      // Installer contract tests can be strengthened after compilation; they
      // are never bundled. All application inputs must still match exactly.
      if (/\.test\.tsx?$/.test(path)) continue;
      assert.equal(hash(`${repo}/${path}`), digest, `Application input changed: ${path}`);
    }
  }
  const win = JSON.parse(readFileSync(`${output}/windows/evidence/${variant}-build-inspection.json`));
  const mac = JSON.parse(readFileSync(`${output}/macos/evidence/${variant}-verification.json`));
  assert.equal(win.version, version);
  assert.equal(mac.version, version);
  assert.equal(win.variant, variant);
  assert.equal(mac.variant, variant);
  assert.equal(mac.dmgNotaryStatus, 'Accepted');
  assert.equal(mac.appNotaryStatus, 'Accepted');
  const origin = variant === 'official' ? 'https://yeschoy.com' : 'https://ai.yeschoy.io';
  assert.equal(win.authorizationPageOrigin, origin);
  assert.equal(mac.authorizationPageOrigin, origin);
  const prefix = `yeschoy-${version}-${variant}`;
  const archive = `${output}/macos/public/${prefix}-macos-universal.app.tar.gz`;
  const entries = execFileSync('tar', ['-tzf', archive], { encoding: 'utf8' }).trim().split('\n');
  assert(entries.every(name => name.startsWith('野菜API.app/') && !name.split('/').includes('..')));
  const extracted = mkdtempSync(`${output}/macos/${variant}-archive-check-`);
  execFileSync('tar', ['-xzf', archive, '-C', extracted]);
  const app = `${extracted}/野菜API.app`;
  assert.equal(hash(`${app}/Contents/MacOS/yeschoy-desktop`), mac.binarySha256);
  execFileSync('codesign', ['--verify', '--deep', '--strict', '--all-architectures', app]);
  execFileSync('xcrun', ['stapler', 'validate', app]);
  for (const [platform, suffix] of [['windows', 'windows-x86_64-installer.exe'], ['macos', 'macos-universal.app.tar.gz'], ['macos', 'macos-universal-installer.dmg']]) {
    const file = `${prefix}-${suffix}`, source = `${output}/${platform}/public/${file}`;
    const digest = hash(source);
    if (suffix.endsWith('.exe')) assert.equal(digest, win.installerSha256);
    if (suffix.endsWith('.dmg')) assert.equal(digest, mac.dmg.sha256);
    copyFileSync(source, `${candidate}/${file}`);
    copyFileSync(`${source}.sig`, `${candidate}/${file}.sig`);
    receipts.push({ file, version, variant, sha256: digest, bytes: statSync(source).size, authorizationPageOrigin: origin });
  }
}
assert.equal(readdirSync(candidate).length, 12);
writeFileSync(`${output}/candidate-receipt.json`, JSON.stringify({ version, files: receipts, installedApplicationsLaunched: false }, null, 2) + '\n');
console.log(JSON.stringify({ candidate, version, fileCount: 12, archivesVerified: true }));
