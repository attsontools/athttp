// @ts-nocheck
import { generateKeyPairSync } from 'node:crypto';
import { readFile } from 'node:fs/promises';
import path from 'node:path';
import { describe, expect, it } from 'vitest';

import {
  createPluginManifest,
  loadPublisherSigningConfig,
  resolveReleaseTarget
} from '../scripts/package-plugin.mjs';

describe('HTTP plugin package contract', () => {
  it('pins the shared SDK release that defines Host API 2.x native services', async () => {
    const packageJson = JSON.parse(
      await readFile(path.resolve(import.meta.dirname, '..', 'package.json'), 'utf8')
    );

    expect(packageJson.dependencies['@attson/plugin-sdk'])
      .toBe('github:attsontools/attool-plugin-sdk#v0.1.0-alpha.1');
  });

  it('binds Host API 2 permissions and the native service to the same signed version', () => {
    const files = {
      'ui/index.html': 'a'.repeat(64),
      'native/athttp-service': 'b'.repeat(64)
    };

    expect(createPluginManifest({
      version: '0.1.0-alpha.0',
      targetOs: 'linux',
      targetArch: 'x86_64',
      publisherKeyId: 'attool-p0-test',
      serviceEntry: 'native/athttp-service',
      files
    })).toEqual({
      schemaVersion: 1,
      appId: 'com.attson.http',
      name: 'HTTP 请求',
      version: '0.1.0-alpha.0',
      publisher: { id: 'com.attson', name: 'Attson', keyId: 'attool-p0-test' },
      target: { os: 'linux', arch: 'x86_64' },
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
        entry: 'native/athttp-service',
        version: '0.1.0-alpha.0',
        sha256: 'b'.repeat(64),
        protocolVersion: 1
      },
      files
    });
  });

  it('rejects a manifest whose target service is absent from the signed files', () => {
    expect(() => createPluginManifest({
      version: '0.1.0-alpha.1',
      targetOs: 'linux',
      targetArch: 'x86_64',
      publisherKeyId: 'attson-http-2026-01',
      serviceEntry: 'native/athttp-service',
      files: { 'ui/index.html': 'a'.repeat(64) }
    })).toThrow(/Native service identity must match the signed package/);
  });

  it('requires matching Ed25519 publisher keys', () => {
    const { privateKey, publicKey } = generateKeyPairSync('ed25519');
    const publicKeyBase64 = Buffer.from(
      publicKey.export({ format: 'jwk' }).x,
      'base64url'
    ).toString('base64');
    const otherPublicKey = generateKeyPairSync('ed25519').publicKey;
    const otherPublicKeyBase64 = Buffer.from(
      otherPublicKey.export({ format: 'jwk' }).x,
      'base64url'
    ).toString('base64');
    const privateKeyPem = privateKey.export({ type: 'pkcs8', format: 'pem' }).toString();

    expect(loadPublisherSigningConfig({
      ATHTTP_PUBLISHER_KEY_ID: 'attson-http-2026-01',
      ATHTTP_PUBLISHER_PRIVATE_KEY: privateKeyPem,
      ATHTTP_PUBLISHER_PUBLIC_KEY: publicKeyBase64
    }).keyId).toBe('attson-http-2026-01');
    expect(() => loadPublisherSigningConfig({
      ATHTTP_PUBLISHER_KEY_ID: 'attson-http-2026-01',
      ATHTTP_PUBLISHER_PRIVATE_KEY: privateKeyPem,
      ATHTTP_PUBLISHER_PUBLIC_KEY: otherPublicKeyBase64
    })).toThrow(/private key does not match.*public key/i);
  });

  it('maps each supported Rust target to its target-specific service binary', () => {
    expect(resolveReleaseTarget('aarch64-unknown-linux-gnu')).toEqual({
      targetTriple: 'aarch64-unknown-linux-gnu',
      targetOs: 'linux',
      targetArch: 'aarch64',
      serviceFileName: 'athttp-service',
      installerExtension: 'deb'
    });
    expect(resolveReleaseTarget('x86_64-pc-windows-msvc')).toEqual({
      targetTriple: 'x86_64-pc-windows-msvc',
      targetOs: 'windows',
      targetArch: 'x86_64',
      serviceFileName: 'athttp-service.exe',
      installerExtension: 'exe'
    });
    expect(() => resolveReleaseTarget('wasm32-unknown-unknown'))
      .toThrow(/Unsupported release target/);
  });
});
