import { spawnSync } from 'node:child_process';
import { readFileSync, writeFileSync, appendFileSync, existsSync, mkdirSync, statSync, readdirSync, readlinkSync } from 'node:fs';
import { createHash } from 'node:crypto';
import { dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import assert from 'node:assert/strict';

const root = process.argv[2];
const [flavor, phase] = process.argv.slice(3);
assert(root && root.startsWith('/'));
const version = JSON.parse(readFileSync(`${root}/evidence/${flavor}-build-inspection.json`)).version;
assert(['official', 'partner'].includes(flavor));
assert(['submit-app', 'status-app', 'app-to-dmg', 'status-dmg', 'verify-dmg'].includes(phase));
const label = flavor === 'official' ? '主版本' : '朋友版';
const app = `${root}/${flavor}-stage/野菜API.app`;
const dmgName = `yeschoy-${version}-${flavor}-macos-universal-installer.dmg`;
const dmg = `${root}/public/${dmgName}`;
const profile = 'yeschoy-notary';
const identity = 'Developer ID Application: xiangguo Zheng (BRG82P5ZB7)';
const logPath = `${root}/evidence/${flavor}-${phase}.log`;
const sha256 = path => createHash('sha256').update(readFileSync(path)).digest('hex');
const save = (name, value) => writeFileSync(`${root}/evidence/${flavor}-${name}.json`, JSON.stringify(value, null, 2) + '\n');
const load = name => JSON.parse(readFileSync(`${root}/evidence/${flavor}-${name}.json`, 'utf8'));
function run(command, args, timeout = 300000) {
  const result = spawnSync(command, args, { encoding: 'utf8', timeout, maxBuffer: 16 * 1024 * 1024 });
  appendFileSync(logPath, JSON.stringify({ at: new Date().toISOString(), command, args, status: result.status, signal: result.signal, error: result.error?.message }) + '\n' + result.stdout + result.stderr + '\n');
  assert.equal(result.status, 0, `${command} failed; see ${logPath}`);
  return result;
}
function submit(kind, path) {
  assert(!existsSync(`${root}/evidence/${flavor}-${kind}-submission.json`), 'Do not duplicate an existing submission');
  const result = JSON.parse(run('xcrun', ['notarytool', 'submit', path, '--keychain-profile', profile, '--output-format', 'json']).stdout);
  assert.match(result.id, /^[a-f0-9-]{36}$/);
  save(`${kind}-submission`, result);
  console.log(JSON.stringify({ flavor, kind, ...result }));
}
function status(kind) {
  const submission = load(`${kind}-submission`);
  const result = JSON.parse(run('xcrun', ['notarytool', 'info', submission.id, '--keychain-profile', profile, '--output-format', 'json'], 60000).stdout);
  assert.equal(result.id, submission.id);
  save(`${kind}-notarization`, result);
  console.log(JSON.stringify({ flavor, kind, ...result }));
  return result;
}
function signatureMetadata(path, universal) {
  const architectures = universal ? ['x86_64', 'arm64'] : [null];
  return architectures.map(architecture => {
    const args = ['--display', '--verbose=4', ...(architecture ? ['--arch', architecture] : []), path];
    const result = run('codesign', args);
    const metadata = result.stdout + result.stderr;
    assert(metadata.includes(`Authority=${identity}`));
    assert(metadata.includes('TeamIdentifier=BRG82P5ZB7'));
    assert(/Timestamp=.+/.test(metadata));
    if (universal) assert(metadata.includes('(runtime)'));
    return { architecture, cdHash: metadata.match(/^CDHash=(.+)$/m)?.[1], signedAt: metadata.match(/^Timestamp=(.+)$/m)?.[1], hardenedRuntime: metadata.includes('(runtime)') };
  });
}

if (phase === 'submit-app') {
  const inspection = load('build-inspection');
  assert.equal(sha256(`${app}/Contents/MacOS/yeschoy-desktop`), inspection.binarySha256);
  run('codesign', ['--verify', '--deep', '--strict', '--all-architectures', app]);
  save('signature-metadata', signatureMetadata(app, true));
  const zip = `${root}/evidence/${flavor}-notary.zip`;
  assert(!existsSync(zip));
  run('ditto', ['-c', '-k', '--keepParent', app, zip]);
  submit('app', zip);
} else if (phase.startsWith('status-')) {
  status(phase.slice(7));
} else if (phase === 'app-to-dmg') {
  assert.equal(load('app-notarization').status, 'Accepted');
  run('xcrun', ['stapler', 'staple', app]);
  run('xcrun', ['stapler', 'validate', app]);
  run('codesign', ['--verify', '--deep', '--strict', '--all-architectures', app]);
  const gatekeeper = run('spctl', ['--assess', '--type', 'execute', '--verbose=2', app]);
  assert((gatekeeper.stdout + gatekeeper.stderr).includes('source=Notarized Developer ID'));
  assert(!existsSync(dmg));
  run('hdiutil', ['create', '-volname', `野菜API ${version} ${label}`, '-srcfolder', `${root}/${flavor}-stage`, '-format', 'UDZO', '-fs', 'HFS+', '-nospotlight', dmg]);
  run('codesign', ['--force', '--sign', identity, '--timestamp', dmg]);
  run('codesign', ['--verify', '--strict', dmg]);
  submit('dmg', dmg);
} else if (phase === 'verify-dmg') {
  assert.equal(load('app-notarization').status, 'Accepted');
  assert.equal(load('dmg-notarization').status, 'Accepted');
  run('xcrun', ['stapler', 'staple', dmg]);
  run('xcrun', ['stapler', 'validate', dmg]);
  run('codesign', ['--verify', '--strict', dmg]);
  const dmgSignature = signatureMetadata(dmg, false);
  const dmgGatekeeper = run('spctl', ['--assess', '--type', 'open', '--context', 'context:primary-signature', '--verbose=2', dmg]);
  assert((dmgGatekeeper.stdout + dmgGatekeeper.stderr).includes('source=Notarized Developer ID'));
  run('hdiutil', ['verify', dmg]);
  const mount = `${root}/${flavor}-mount`;
  mkdirSync(mount, { recursive: true });
  assert.equal(readdirSync(mount).length, 0);
  let mounted = false;
  let mountedArchitecture;
  try {
    run('hdiutil', ['attach', '-readonly', '-nobrowse', '-mountpoint', mount, dmg]);
    mounted = true;
    const mountedApp = `${mount}/野菜API.app`;
    run('codesign', ['--verify', '--deep', '--strict', '--all-architectures', mountedApp]);
    run('xcrun', ['stapler', 'validate', mountedApp]);
    mountedArchitecture = run('lipo', ['-archs', `${mountedApp}/Contents/MacOS/yeschoy-desktop`]).stdout.trim().split(/\s+/).sort();
    assert.deepEqual(mountedArchitecture, ['arm64', 'x86_64']);
    for (const file of ['Contents/MacOS/yeschoy-desktop', 'Contents/Info.plist', 'Contents/_CodeSignature/CodeResources']) {
      assert.equal(sha256(`${mountedApp}/${file}`), sha256(`${app}/${file}`));
    }
    assert.equal(readlinkSync(`${mount}/Applications`), '/Applications');
  } finally {
    if (mounted) run('hdiutil', ['detach', mount]);
  }
  const inspection = load('build-inspection');
  assert.equal(sha256(`${app}/Contents/MacOS/yeschoy-desktop`), inspection.binarySha256);
  const result = { flavor, ...inspection, appNotarySubmissionId: load('app-notarization').id, appNotaryStatus: 'Accepted', appCodeSignature: 'passed all architectures', appStaple: 'validated', appGatekeeper: 'accepted: Notarized Developer ID', appSignature: load('signature-metadata'), dmgNotarySubmissionId: load('dmg-notarization').id, dmgNotaryStatus: 'Accepted', dmgCodeSignature: 'passed', dmgStaple: 'validated', dmgGatekeeper: 'accepted: Notarized Developer ID', dmgSignature, dmgChecksum: 'verified', mountedPayloadSignature: 'passed all architectures', mountedPayloadStaple: 'validated', mountedPayloadArchitecture: mountedArchitecture, mountedPayloadMatchesStaging: true, mountedImageDetached: true, dmg: { file: dmgName, bytes: statSync(dmg).size, sha256: sha256(dmg) }, installedApplicationsLaunchedOrConfigured: false, finalRuntimeSmokeTestPerformed: false, verifiedAt: new Date().toISOString() };
  save('verification', result);
  console.log(JSON.stringify(result, null, 2));
}

