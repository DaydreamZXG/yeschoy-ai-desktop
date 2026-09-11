// Local release credentials never enter the repository, public directory or logs.
import { execFileSync } from 'node:child_process';
import { chmodSync, copyFileSync, existsSync, lstatSync, mkdirSync, readFileSync, constants } from 'node:fs';
import { homedir } from 'node:os';
import { resolve } from 'node:path';

const root = resolve(import.meta.dirname, '../..');
const directory = resolve(homedir(), '.config/yeschoy-release/updater-v2');
const backup = resolve(homedir(), '.config/yeschoy-release/updater-v2-backup');
const [action, variant, artifact] = process.argv.slice(2);
if (!['init', 'sign'].includes(action) || !['official', 'partner'].includes(variant)) {
  throw new Error('Usage: node local-signing.mjs init|sign official|partner [artifact]');
}
process.umask(0o077);
for (const path of [directory, backup]) {
  mkdirSync(path, { recursive: true, mode: 0o700 });
  if (lstatSync(path).isSymbolicLink()) throw new Error('Signing directory cannot be a symlink');
  chmodSync(path, 0o700);
}
const key = `${directory}/${variant}.key`;
if (!existsSync(key) && existsSync(`${backup}/${variant}.key`)) {
  throw new Error('A key backup exists; restore the matching key instead of generating a new one');
}
function tauri(args) {
  try {
    execFileSync('pnpm', ['exec', 'tauri', 'signer', ...args], {
      cwd: root, stdio: ['ignore', 'pipe', 'pipe'], timeout: 120000,
      env: { ...process.env, TAURI_SIGNING_PRIVATE_KEY_PASSWORD: '' },
    });
  } catch {
    // Signing tools may print sensitive inputs in diagnostics. Do not relay them.
    throw new Error('Local signing command failed; no credential output retained');
  }
}
if (action === 'init' && !existsSync(key)) tauri(['generate', '--ci', '-p', '', '-w', key]);
if (!existsSync(key) || lstatSync(key).isSymbolicLink()) throw new Error('Missing regular local signing key');
chmodSync(key, 0o600);
const publicKey = readFileSync(`${key}.pub`, 'utf8').trim();
if (!publicKey || publicKey.length > 4096) throw new Error('Invalid public key');
for (const suffix of ['', '.pub']) {
  const destination = `${backup}/${variant}.key${suffix}`;
  if (!existsSync(destination)) copyFileSync(`${key}${suffix}`, destination, constants.COPYFILE_EXCL);
  if (lstatSync(destination).isSymbolicLink() || !readFileSync(destination).equals(readFileSync(`${key}${suffix}`))) {
    throw new Error('Existing key backup differs; refusing to overwrite');
  }
  chmodSync(destination, 0o600);
}
if (action === 'sign') {
  const registry = JSON.parse(readFileSync(`${root}/src-tauri/update-channels.json`, 'utf8'));
  if (registry[variant]?.publicKey !== publicKey) throw new Error('Signing key does not match the compiled channel');
  if (!artifact || !lstatSync(artifact).isFile() || lstatSync(artifact).isSymbolicLink()) throw new Error('Regular artifact required');
  if (existsSync(`${artifact}.sig`)) throw new Error('Refusing to replace existing signature');
  tauri(['sign', '-f', key, '-p', '', resolve(artifact)]);
}
console.log(JSON.stringify({ variant, publicKey, keyDirectory: directory, localBackupDirectory: backup, action }));
