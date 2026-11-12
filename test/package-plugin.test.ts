import { describe, expect, it } from 'vitest';

// @ts-expect-error The packaging script is plain Node ESM.
import { createPluginManifest } from '../scripts/package-plugin.mjs';

describe('HTTP plugin package manifest', () => {
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
});
