import { spawnSync, execFileSync } from 'node:child_process';
import { readFileSync, writeFileSync, readdirSync, statSync, existsSync, mkdirSync, copyFileSync, symlinkSync, appendFileSync } from 'node:fs';
import { resolve, join } from 'node:path';
import { homedir } from 'node:os';
import { createHash } from 'node:crypto';
import assert from 'node:assert/strict';

const repo = resolve(import.meta.dirname, '../..');
const [platform, variant, requestedOutput] = process.argv.slice(2);
assert(['windows', 'macos', 'macos-finalize'].includes(platform));
assert(['official', 'partner'].includes(variant));
assert(requestedOutput);
const output = resolve(requestedOutput);
assert(output.startsWith(`${repo}/release/internal/`));
const version = JSON.parse(readFileSync(`${repo}/package.json`)).version;
assert.equal(version, JSON.parse(readFileSync(`${repo}/src-tauri/tauri.conf.json`)).version);
assert.match(version, /^\d+\.\d+\.\d+$/);
const origin = variant === 'official' ? 'https://yeschoy.com' : 'https://ai.yeschoy.io';
const prefix = `yeschoy-${version}-${variant}`;
const folder = `${output}/${platform === 'windows' ? 'windows' : 'macos'}`;
mkdirSync(`${folder}/public`, { recursive: true });
mkdirSync(`${folder}/evidence`, { recursive: true });
const log = `${folder}/evidence/${variant}-${platform}-build.log`;
const hash = path => createHash('sha256').update(readFileSync(path)).digest('hex');
const save = (name, value) => writeFileSync(`${folder}/evidence/${variant}-${name}.json`, JSON.stringify(value, null, 2) + '\n');
const env = { ...process.env, RUSTUP_TOOLCHAIN: '1.94.0', YESCHOY_AUTHORIZATION_PAGE_ORIGIN: origin };
for (const name of ['RUSTFLAGS', 'CARGO_ENCODED_RUSTFLAGS', 'CARGO_TARGET_DIR', 'TAURI_CONFIG', 'TAURI_SIGNING_PRIVATE_KEY', 'TAURI_SIGNING_PRIVATE_KEY_PASSWORD', 'APPLE_ID', 'APPLE_PASSWORD', 'APPLE_TEAM_ID', 'APPLE_API_KEY', 'APPLE_API_ISSUER', 'APPLE_API_KEY_PATH', 'APPLE_CERTIFICATE', 'APPLE_CERTIFICATE_PASSWORD', 'APPLE_SIGNING_IDENTITY']) delete env[name];
function run(command, args, overrides = {}, timeout = 1800000) {
  const result = spawnSync(command, args, { cwd: repo, env: { ...env, ...overrides }, encoding: 'utf8', maxBuffer: 16 * 1024 * 1024, timeout });
  appendFileSync(log, `${command} ${JSON.stringify(args)}\n${result.stdout || ''}${result.stderr || ''}\n`);
  assert.equal(result.status, 0, `Build step failed; see ${log}`);
  return result.stdout;
}
function snapshot() {
  const files = execFileSync('git', ['ls-files', '-c', '-o', '--exclude-standard'], { cwd: repo, encoding: 'utf8' }).trim().split('\n')
    .filter(p => /^(src\/|src-tauri\/|package.json$|pnpm-lock.yaml$|vite.config|tsconfig)/.test(p) && !p.startsWith('src-tauri/target/'));
  return Object.fromEntries([...new Set(files)].sort().map(p => [p, hash(`${repo}/${p}`)]));
}
function buildEvidence(target) {
  const base = `${repo}/src-tauri/target/${target}/release/build`;
  const outputs = readdirSync(base).filter(n => n.startsWith('yeschoy-desktop-')).map(n => join(base, n, 'output'))
    .filter(p => existsSync(p)).sort((a, b) => statSync(b).mtimeMs - statSync(a).mtimeMs);
  const latest = outputs[0];
  assert(latest);
  assert(readFileSync(latest, 'utf8').split(/\r?\n/).includes(`cargo:rustc-env=YESCHOY_AUTHORIZATION_PAGE_ORIGIN=${origin}`));
  copyFileSync(latest, `${folder}/evidence/${variant}-${target}-build-output.txt`);
  return { target, authorizationPageOrigin: origin, buildOutputSha256: hash(latest), output: latest };
}
function sign(path) {
  run('node', [`${repo}/deploy/self-update/local-signing.mjs`, 'sign', variant, path]);
}
const before = snapshot();
const commit = execFileSync('git', ['rev-parse', 'HEAD'], { cwd: repo, encoding: 'utf8' }).trim();
if (platform === 'windows') {
  const destination = `${folder}/public/${prefix}-windows-x86_64-installer.exe`;
  assert(!existsSync(destination), 'Refusing to replace an existing installer');
  const zig = process.env.YESCHOY_ZIG || `${homedir()}/.cache/uv/archive-v0/9UrQOgeLIxroVAWO/ziglang/zig`;
  run(`${homedir()}/.local/bin/cargo-xwin`, ['build', '--manifest-path', 'src-tauri/Cargo.toml', '--release', '--target', 'x86_64-pc-windows-msvc', '--bin', 'yeschoy-desktop', '--features', 'custom-protocol', '--locked', '--offline'], {
    XWIN_CROSS_COMPILER: 'clang', STATIC_VCRUNTIME: 'false', RUSTFLAGS: '-C target-feature=+crt-static -C target-feature=+crt-static',
    CARGO_TARGET_DIR: `${repo}/src-tauri/target`, RC: `${repo}/deploy/self-update/llvm-rc`, YESCHOY_ZIG: zig,
  });
  const build = buildEvidence('x86_64-pc-windows-msvc');
  const resource = resolve(build.output, '../out/resource.rc');
  assert(readFileSync(resource, 'utf8').includes(`FILEVERSION ${version.split('.').join(', ')}, 0`));
  assert(statSync(resolve(resource, '../resource.lib')).size > 0);
  const exe = `${repo}/src-tauri/target/x86_64-pc-windows-msvc/release/yeschoy-desktop.exe`;
  const binary = readFileSync(exe), pe = binary.readUInt32LE(0x3c);
  assert.equal(binary.readUInt16LE(pe + 4), 0x8664);
  assert.equal(binary.readUInt16LE(pe + 24 + 68), 2);
  assert(binary.includes(Buffer.from(version, 'utf16le')));
  const payload = `${folder}/${prefix}-payload.exe`;
  copyFileSync(exe, payload);
  const nsis = `${repo}/release/internal/0.4.15-partner-ai-yeschoy-04382354/tooling/makensis/3.12`;
  run(`${nsis}/bin/makensis`, [`-DAPP_EXE=${payload}`, `-DOUTPUT_EXE=${destination}`, `-DAPP_ICON=${repo}/src-tauri/icons/icon.ico`, `-DAPP_VERSION=${version}`, 'src-tauri/windows/installer.nsi'], { NSISDIR: `${nsis}/share/nsis` });
  sign(destination);
  save('build-inspection', { platform, variant, version, commit, authorizationPageOrigin: origin, build, resourceSha256: hash(resource), payloadSha256: hash(payload), installerSha256: hash(destination), codeSigned: false, updaterSignatureCreated: true, installedApplicationLaunched: false });
} else if (platform === 'macos') {
  const stage = `${folder}/${variant}-stage`;
  assert(!existsSync(`${stage}/野菜API.app`));
  const config = `${folder}/build-config.json`;
  writeFileSync(config, JSON.stringify({ bundle: { active: true, createUpdaterArtifacts: false, macOS: { hardenedRuntime: true, signingIdentity: 'Developer ID Application: xiangguo Zheng (BRG82P5ZB7)' } } }));
  run('pnpm', ['exec', 'tauri', 'build', '--ci', '--target', 'universal-apple-darwin', '--bundles', 'app', '--config', config, '--', '--locked', '--offline']);
  mkdirSync(stage, { recursive: true });
  const app = `${stage}/野菜API.app`;
  run('ditto', [`${repo}/src-tauri/target/universal-apple-darwin/release/bundle/macos/野菜API.app`, app]);
  symlinkSync('/Applications', `${stage}/Applications`);
  run('codesign', ['--verify', '--deep', '--strict', '--all-architectures', app]);
  const binary = `${app}/Contents/MacOS/yeschoy-desktop`;
  const architectures = run('lipo', ['-archs', binary]).trim().split(/\s+/).sort();
  assert.deepEqual(architectures, ['arm64', 'x86_64']);
  const plist = JSON.parse(run('plutil', ['-convert', 'json', '-o', '-', `${app}/Contents/Info.plist`]));
  assert.equal(plist.CFBundleShortVersionString, version);
  assert.equal(plist.CFBundleIdentifier, 'com.yeschoy.desktop');
  save('build-inspection', { flavor: variant, variant, version, commit, authorizationPageOrigin: origin, appPath: app, architectures, binarySha256: hash(binary), builds: ['x86_64-apple-darwin', 'aarch64-apple-darwin'].map(buildEvidence), installedApplicationLaunched: false });
} else {
  const verification = JSON.parse(readFileSync(`${folder}/evidence/${variant}-verification.json`));
  assert.equal(verification.dmgNotaryStatus, 'Accepted');
  assert.equal(verification.version, version);
  const tar = `${folder}/public/${prefix}-macos-universal.app.tar.gz`;
  assert(!existsSync(tar));
  run('/usr/bin/tar', ['-czf', tar, '-C', `${folder}/${variant}-stage`, '野菜API.app'], { COPYFILE_DISABLE: '1' });
  sign(tar);
  sign(`${folder}/public/${prefix}-macos-universal-installer.dmg`);
}
assert.deepEqual(snapshot(), before, 'Application source changed during build');
save(`${platform}-source`, { commit, version, variant, pathHashes: before, sourceSha256: createHash('sha256').update(JSON.stringify(before)).digest('hex') });
console.log(JSON.stringify({ platform, variant, version, output: folder, status: 'built', runtimeSmokeTestPerformed: false }));
