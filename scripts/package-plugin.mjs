import { createHash, createPrivateKey, createPublicKey } from 'node:crypto';
import {
  cp,
  mkdir,
  mkdtemp,
  readFile,
  readdir,
  rm,
  writeFile
} from 'node:fs/promises';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { pathToFileURL } from 'node:url';

import { parseManifest } from '@attson/plugin-sdk';
import { signManifest, verifyManifestSignature } from '@attson/plugin-sdk/node';

const RELEASE_TARGETS = {
  'aarch64-apple-darwin': ['macos', 'aarch64', 'athttp-service', 'dmg'],
  'x86_64-apple-darwin': ['macos', 'x86_64', 'athttp-service', 'dmg'],
  'aarch64-unknown-linux-gnu': ['linux', 'aarch64', 'athttp-service', 'deb'],
  'x86_64-unknown-linux-gnu': ['linux', 'x86_64', 'athttp-service', 'deb'],
  'x86_64-pc-windows-msvc': ['windows', 'x86_64', 'athttp-service.exe', 'exe']
};

export function resolveReleaseTarget(targetTriple) {
  const target = RELEASE_TARGETS[targetTriple];
  if (!target) throw new Error(`Unsupported release target: ${targetTriple}`);
  return {
    targetTriple,
    targetOs: target[0],
    targetArch: target[1],
    serviceFileName: target[2],
    installerExtension: target[3]
  };
}

export function createPluginManifest({
  version,
  targetOs,
  targetArch,
  publisherKeyId,
  serviceEntry,
  files
}) {
  return parseManifest({
    schemaVersion: 1,
    appId: 'com.attson.http',
    name: 'HTTP 请求',
    version,
    publisher: { id: 'com.attson', name: 'Attson', keyId: publisherKeyId },
    target: { os: targetOs, arch: targetArch },
    hostApi: { min: '2.0.0', max: '2.x' },
    ui: { entry: 'ui/index.html' },
    permissions: [
      'host.theme.read',
      'host.file.save',
      'host.http.request',
      'host.http.stream',
      'host.http.data'
    ],
    nativeService: {
      entry: serviceEntry,
      version,
      sha256: files[serviceEntry],
      protocolVersion: 1
    },
    files
  });
}

export async function buildPluginPackage({ root, version, signing, targetTriple }) {
  const target = resolveReleaseTarget(targetTriple);
  const stage = path.join(root, '.plugin-stage');
  const artifacts = path.join(root, 'artifacts');
  const output = path.join(
    artifacts,
    `AT.HTTP_${version}_${target.targetOs}_${target.targetArch}.atpkg`
  );
  const serviceEntry = `native/${target.serviceFileName}`;
  await rm(stage, { recursive: true, force: true });
  await rm(output, { force: true });
  try {
    await run(
      'cargo',
      ['build', '--release', '--bin', 'athttp-service', '--target', targetTriple],
      root
    );
    await mkdir(path.join(stage, 'ui'), { recursive: true });
    await mkdir(path.join(stage, 'native'), { recursive: true });
    await mkdir(artifacts, { recursive: true });
    await cp(path.join(root, 'dist-plugin'), path.join(stage, 'ui'), { recursive: true });
    await cp(path.join(root, 'LICENSE'), path.join(stage, 'LICENSE'));
    await cp(
      path.join(root, 'target', targetTriple, 'release', target.serviceFileName),
      path.join(stage, serviceEntry)
    );

    const files = {};
    for (const relative of await collect(stage)) {
      files[relative] = sha256(await readFile(path.join(stage, relative)));
    }
    const manifest = createPluginManifest({
      version,
      targetOs: target.targetOs,
      targetArch: target.targetArch,
      publisherKeyId: signing.keyId,
      serviceEntry,
      files
    });
    await writeFile(path.join(stage, 'manifest.json'), `${JSON.stringify(manifest, null, 2)}\n`);
    await writeFile(
      path.join(stage, 'manifest.sig'),
      `${signManifest(manifest, signing.privateKey)}\n`
    );
    await archive(stage, output);
    await verifyPluginPackage(output, signing.publicKey, {
      version,
      publisherKeyId: signing.keyId,
      ...target
    });
    return output;
  } catch (error) {
    await rm(output, { force: true });
    throw error;
  } finally {
    await rm(stage, { recursive: true, force: true });
  }
}

export function loadPublisherSigningConfig(env) {
  const keyId = env.ATHTTP_PUBLISHER_KEY_ID?.trim();
  if (!keyId) throw new Error('ATHTTP_PUBLISHER_KEY_ID is required');
  const encodedKey = env.ATHTTP_PUBLISHER_PRIVATE_KEY?.trim();
  if (!encodedKey) throw new Error('ATHTTP_PUBLISHER_PRIVATE_KEY is required');
  const encodedPublicKey = env.ATHTTP_PUBLISHER_PUBLIC_KEY?.trim();
  if (!encodedPublicKey) throw new Error('ATHTTP_PUBLISHER_PUBLIC_KEY is required');
  if (!/^[A-Za-z0-9+/]{43}=$/.test(encodedPublicKey)) {
    throw new Error('ATHTTP_PUBLISHER_PUBLIC_KEY must be a raw Ed25519 public key');
  }

  let privateKey;
  try {
    privateKey = createPrivateKey(encodedKey);
  } catch {
    throw new Error('ATHTTP_PUBLISHER_PRIVATE_KEY must be an Ed25519 private key');
  }
  if (privateKey.asymmetricKeyType !== 'ed25519') {
    throw new Error('ATHTTP_PUBLISHER_PRIVATE_KEY must be an Ed25519 private key');
  }
  const publicKey = createPublicKey(privateKey);
  const derivedPublicKey = publicKey.export({ format: 'jwk' });
  const derivedPublicKeyBase64 = Buffer.from(derivedPublicKey.x, 'base64url').toString('base64');
  if (derivedPublicKeyBase64 !== encodedPublicKey) {
    throw new Error('Publisher private key does not match configured public key');
  }
  return { keyId, privateKey, publicKey };
}

export async function verifyPluginPackage(packagePath, publicKey, expected) {
  const expectedName =
    `AT.HTTP_${expected.version}_${expected.targetOs}_${expected.targetArch}.atpkg`;
  if (path.basename(packagePath) !== expectedName) {
    throw new Error(`Package file name must be ${expectedName}`);
  }
  resolveReleaseTarget(expected.targetTriple);

  const extracted = await mkdtemp(path.join(tmpdir(), 'athttp-verify-'));
  try {
    await extract(packagePath, extracted);
    const manifest = parseManifest(
      JSON.parse(await readFile(path.join(extracted, 'manifest.json'), 'utf8'))
    );
    const signature = (await readFile(path.join(extracted, 'manifest.sig'), 'utf8')).trim();
    if (!verifyManifestSignature(manifest, signature, publicKey)) {
      throw new Error('Plugin manifest signature is invalid');
    }
    requireEqual(manifest.appId, 'com.attson.http', 'app ID');
    requireEqual(manifest.version, expected.version, 'version');
    requireEqual(manifest.publisher.keyId, expected.publisherKeyId, 'Publisher Key ID');
    requireEqual(manifest.target.os, expected.targetOs, 'target OS');
    requireEqual(manifest.target.arch, expected.targetArch, 'target architecture');
    requireEqual(
      manifest.nativeService?.entry,
      `native/${expected.serviceFileName}`,
      'native service entry'
    );

    const actualFiles = (await collect(extracted))
      .filter((relative) => relative !== 'manifest.json' && relative !== 'manifest.sig');
    const declaredFiles = Object.keys(manifest.files).sort();
    if (JSON.stringify(actualFiles) !== JSON.stringify(declaredFiles)) {
      throw new Error('Package files do not match the manifest');
    }
    for (const relative of declaredFiles) {
      const actualHash = sha256(await readFile(path.join(extracted, relative)));
      if (actualHash !== manifest.files[relative]) {
        throw new Error(`Package hash mismatch for ${relative}`);
      }
    }
    return manifest;
  } finally {
    await rm(extracted, { recursive: true, force: true });
  }
}

async function main() {
  const root = path.resolve(import.meta.dirname, '..');
  const packageJson = JSON.parse(await readFile(path.join(root, 'package.json'), 'utf8'));
  const signing = loadPublisherSigningConfig(process.env);
  const targetTriple = process.env.ATHTTP_RELEASE_TARGET?.trim() || rustHostTarget();
  const output = await buildPluginPackage({
    root,
    version: packageJson.version,
    signing,
    targetTriple
  });
  console.log(output);
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  await main();
}

async function collect(directory, prefix = '') {
  const output = [];
  for (const entry of await readdir(path.join(directory, prefix), { withFileTypes: true })) {
    const relative = path.posix.join(prefix, entry.name);
    if (entry.isDirectory()) output.push(...await collect(directory, relative));
    else output.push(relative);
  }
  return output.sort();
}

function rustHostTarget() {
  const result = spawnSync('rustc', ['-vV'], { encoding: 'utf8' });
  if (result.status !== 0) throw new Error('rustc -vV failed');
  const target = result.stdout.match(/^host: (.+)$/m)?.[1];
  if (!target) throw new Error('Unable to determine Rust host target');
  return target;
}

function sha256(content) {
  return createHash('sha256').update(content).digest('hex');
}

async function archive(stage, output) {
  const entries = await collect(stage);
  const command = process.platform === 'win32' ? '7z' : 'zip';
  const args = process.platform === 'win32'
    ? ['a', '-tzip', '-mx=9', output, ...entries]
    : ['-X', '-D', '-q', output, ...entries];
  await run(command, args, stage);
}

async function extract(packagePath, destination) {
  const command = process.platform === 'win32' ? '7z' : 'unzip';
  const args = process.platform === 'win32'
    ? ['x', '-y', `-o${destination}`, packagePath]
    : ['-q', packagePath, '-d', destination];
  await run(command, args, process.cwd());
}

function requireEqual(actual, expected, label) {
  if (actual !== expected) {
    throw new Error(`Package ${label} mismatch: expected ${expected}, received ${actual}`);
  }
}

async function run(command, args, cwd) {
  const result = spawnSync(command, args, { cwd, stdio: 'inherit' });
  if (result.status !== 0) throw new Error(`${command} failed`);
}
