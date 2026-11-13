import { readFile, readdir, stat } from 'node:fs/promises';
import path from 'node:path';
import { pathToFileURL } from 'node:url';

const TARGETS = [
  ['linux', 'aarch64', 'deb'],
  ['linux', 'x86_64', 'deb'],
  ['macos', 'aarch64', 'dmg'],
  ['macos', 'x86_64', 'dmg'],
  ['windows', 'x86_64', 'exe']
];

export function assertVersionAlignment({
  tagName,
  packageVersion,
  tauriVersion,
  serviceCargoVersion,
  tauriCargoVersion
}) {
  const version = versionFromTag(tagName);
  if (
    [packageVersion, tauriVersion, serviceCargoVersion, tauriCargoVersion]
      .some((candidate) => candidate !== version)
  ) {
    throw new Error(`package, Tauri, service Cargo, and app Cargo versions must all match tag ${tagName}`);
  }
  return version;
}

export function expectedReleaseAssets(version) {
  requireSemver(version);
  return TARGETS.flatMap(([os, arch, extension]) => [
    `AT.HTTP_${version}_${os}_${arch}.atpkg`,
    `AT.HTTP_${version}_${os}_${arch}.${extension}`
  ]).sort();
}

export async function verifyReleaseAssets(directory, version) {
  const expected = expectedReleaseAssets(version);
  const entries = await readdir(directory, { withFileTypes: true });
  const actual = entries.map((entry) => entry.name).sort();
  if (JSON.stringify(actual) !== JSON.stringify(expected) || entries.some((entry) => !entry.isFile())) {
    throw new Error(
      `Release directory must contain the exact release asset set:\n${expected.join('\n')}`
    );
  }
  for (const fileName of actual) {
    if ((await stat(path.join(directory, fileName))).size < 1) {
      throw new Error(`Release asset is empty: ${fileName}`);
    }
  }
}

async function verifyProjectVersions(root, tagName) {
  const packageJson = JSON.parse(await readFile(path.join(root, 'package.json'), 'utf8'));
  const tauriConfig = JSON.parse(
    await readFile(path.join(root, 'src-tauri', 'tauri.conf.json'), 'utf8')
  );
  const serviceCargoVersion = await readCargoVersion(path.join(root, 'Cargo.toml'));
  const tauriCargoVersion = await readCargoVersion(path.join(root, 'src-tauri', 'Cargo.toml'));
  return assertVersionAlignment({
    tagName,
    packageVersion: packageJson.version,
    tauriVersion: tauriConfig.version,
    serviceCargoVersion,
    tauriCargoVersion
  });
}

async function readCargoVersion(filePath) {
  const cargoToml = await readFile(filePath, 'utf8');
  const version = /^\[package\][\s\S]*?^version\s*=\s*"([^"]+)"/m.exec(cargoToml)?.[1];
  if (!version) throw new Error(`Unable to read Cargo package version from ${filePath}`);
  return version;
}

function versionFromTag(tagName) {
  if (typeof tagName !== 'string' || !tagName.startsWith('v')) {
    throw new Error('Release tag must be a v-prefixed SemVer');
  }
  const version = tagName.slice(1);
  requireSemver(version);
  return version;
}

function requireSemver(version) {
  if (!/^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(?:-((?:0|[1-9]\d*|[0-9]*[A-Za-z-][0-9A-Za-z-]*)(?:\.(?:0|[1-9]\d*|[0-9]*[A-Za-z-][0-9A-Za-z-]*))*))?(?:\+[0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*)?$/.test(version)) {
    throw new Error(`Invalid SemVer: ${version}`);
  }
}

async function main([command, ...args]) {
  if (command === 'versions' && args.length === 1) {
    const version = await verifyProjectVersions(path.resolve(import.meta.dirname, '..'), args[0]);
    console.log(`Release versions match ${version}`);
    return;
  }
  if (command === 'assets' && args.length === 2) {
    await verifyReleaseAssets(args[0], args[1]);
    console.log(`Release asset set matches ${args[1]}`);
    return;
  }
  throw new Error('Usage: verify-release.mjs versions <tag> | assets <directory> <version>');
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  await main(process.argv.slice(2));
}
