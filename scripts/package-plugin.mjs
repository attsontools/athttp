import {
  createHash,
  createPrivateKey,
  createPublicKey,
  sign,
  verify
} from 'node:crypto';
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

export function createPluginManifest({
  version,
  targetOs,
  targetArch,
  publisherKeyId,
  serviceEntry,
  files
}) {
  return {
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
  };
}

export async function buildPluginPackage({ root, version, signing, targetOs, targetArch }) {
  const stage = path.join(root, '.plugin-stage');
  const artifacts = path.join(root, 'artifacts');
  const output = path.join(artifacts, `AT.HTTP_${version}_${targetOs}_${targetArch}.atpkg`);
  const extension = targetOs === 'windows' ? '.exe' : '';
  const serviceEntry = `native/athttp-service${extension}`;
  await rm(stage, { recursive: true, force: true });
  await rm(output, { force: true });
  try {
    await run('cargo', ['build', '--release', '--bin', 'athttp-service'], root);
    await mkdir(path.join(stage, 'ui'), { recursive: true });
    await mkdir(path.join(stage, 'native'), { recursive: true });
    await mkdir(artifacts, { recursive: true });
    await cp(path.join(root, 'dist-plugin'), path.join(stage, 'ui'), { recursive: true });
    await cp(
      path.join(root, 'target', 'release', `athttp-service${extension}`),
      path.join(stage, serviceEntry)
    );

    const files = {};
    for (const relative of await collect(stage)) {
      files[relative] = sha256(await readFile(path.join(stage, relative)));
    }
    const manifest = createPluginManifest({
      version,
      targetOs,
      targetArch,
      publisherKeyId: signing.keyId,
      serviceEntry,
      files
    });
    const canonical = canonicalJson(manifest);
    const signature = sign(null, Buffer.from(canonical), signing.privateKey).toString('base64');
    await writeFile(path.join(stage, 'manifest.json'), `${JSON.stringify(manifest, null, 2)}\n`);
    await writeFile(path.join(stage, 'manifest.sig'), `${signature}\n`);
    await archive(stage, output);
    await verifyPluginPackage(output, createPublicKey(signing.privateKey), manifest);
    return output;
  } catch (error) {
    await rm(output, { force: true });
    throw error;
  } finally {
    await rm(stage, { recursive: true, force: true });
  }
}

export function loadSigningConfig(env) {
  const keyId = env.ATHTTP_PUBLISHER_KEY_ID?.trim();
  const encodedKey = env.ATHTTP_PUBLISHER_PRIVATE_KEY?.trim();
  if (!keyId) throw new Error('ATHTTP_PUBLISHER_KEY_ID is required');
  if (!encodedKey) throw new Error('ATHTTP_PUBLISHER_PRIVATE_KEY is required');
  let privateKey;
  try {
    privateKey = createPrivateKey(encodedKey);
  } catch {
    throw new Error('ATHTTP_PUBLISHER_PRIVATE_KEY must be an Ed25519 private key');
  }
  if (privateKey.asymmetricKeyType !== 'ed25519') {
    throw new Error('ATHTTP_PUBLISHER_PRIVATE_KEY must be an Ed25519 private key');
  }
  return { keyId, privateKey };
}

export async function verifyPluginPackage(packagePath, publicKey, expectedManifest) {
  const extracted = await mkdtemp(path.join(tmpdir(), 'athttp-verify-'));
  try {
    await extract(packagePath, extracted);
    const manifest = JSON.parse(await readFile(path.join(extracted, 'manifest.json'), 'utf8'));
    const signature = (await readFile(path.join(extracted, 'manifest.sig'), 'utf8')).trim();
    if (canonicalJson(manifest) !== canonicalJson(expectedManifest)) {
      throw new Error('Plugin manifest changed during packaging');
    }
    if (!verify(null, Buffer.from(canonicalJson(manifest)), publicKey, Buffer.from(signature, 'base64'))) {
      throw new Error('Plugin manifest signature is invalid');
    }
    const actualFiles = (await collect(extracted))
      .filter((relative) => relative !== 'manifest.json' && relative !== 'manifest.sig');
    if (JSON.stringify(actualFiles) !== JSON.stringify(Object.keys(manifest.files).sort())) {
      throw new Error('Package files do not match the manifest');
    }
    for (const relative of actualFiles) {
      if (sha256(await readFile(path.join(extracted, relative))) !== manifest.files[relative]) {
        throw new Error(`Package hash mismatch for ${relative}`);
      }
    }
  } finally {
    await rm(extracted, { recursive: true, force: true });
  }
}

async function main() {
  const root = path.resolve(import.meta.dirname, '..');
  const packageJson = JSON.parse(await readFile(path.join(root, 'package.json'), 'utf8'));
  const targetOs = { darwin: 'macos', win32: 'windows', linux: 'linux' }[process.platform];
  const targetArch = { arm64: 'aarch64', x64: 'x86_64' }[process.arch];
  if (!targetOs || !targetArch) throw new Error(`Unsupported target: ${process.platform}/${process.arch}`);
  const output = await buildPluginPackage({
    root,
    version: packageJson.version,
    signing: loadSigningConfig(process.env),
    targetOs,
    targetArch
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

function canonicalJson(value) {
  if (Array.isArray(value)) return `[${value.map(canonicalJson).join(',')}]`;
  if (value && typeof value === 'object') {
    return `{${Object.keys(value).sort().map((key) =>
      `${JSON.stringify(key)}:${canonicalJson(value[key])}`
    ).join(',')}}`;
  }
  return JSON.stringify(value);
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

async function run(command, args, cwd) {
  const result = spawnSync(command, args, { cwd, stdio: 'inherit' });
  if (result.status !== 0) throw new Error(`${command} failed`);
}
