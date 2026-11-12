import { execFileSync } from 'node:child_process';
import { chmodSync, copyFileSync, mkdirSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const release = process.argv.includes('--release');
const target = process.env.TAURI_ENV_TARGET_TRIPLE || rustHostTarget();
const extension = target.includes('windows') ? '.exe' : '';
const profile = release ? 'release' : 'debug';
const cargoArgs = ['build', '--bin', 'athttp-service', '--target', target];
if (release) cargoArgs.push('--release');
execFileSync('cargo', cargoArgs, { cwd: root, stdio: 'inherit' });

const source = resolve(root, 'target', target, profile, `athttp-service${extension}`);
const destination = resolve(
  root,
  'src-tauri',
  'binaries',
  `athttp-service-${target}${extension}`
);
mkdirSync(dirname(destination), { recursive: true });
copyFileSync(source, destination);
if (!extension) chmodSync(destination, 0o755);

function rustHostTarget() {
  const output = execFileSync('rustc', ['-vV'], { encoding: 'utf8' });
  const match = output.match(/^host: (.+)$/m);
  if (!match) throw new Error('Unable to determine Rust host target');
  return match[1];
}
