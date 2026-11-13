// @ts-nocheck
import { mkdtemp, readFile, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { describe, expect, it } from 'vitest';

import {
  assertVersionAlignment,
  expectedReleaseAssets,
  verifyReleaseAssets
} from '../scripts/verify-release.mjs';

const version = '0.1.0-alpha.1';

describe('HTTP production release contract', () => {
  it('runs tag version verification with Bash on every matrix platform', async () => {
    const workflow = await readFile(
      path.resolve(import.meta.dirname, '..', '.github', 'workflows', 'release.yml'),
      'utf8'
    );

    expect(workflow).toMatch(
      /- name: Verify release version alignment\n\s+if: [^\n]+\n\s+shell: bash\n\s+run: node scripts\/verify-release\.mjs versions "\$\{GITHUB_REF_NAME\}"/
    );
    expect(workflow).toMatch(/\n\s+perl .*' Cargo\.toml\n/);
    expect(workflow).toMatch(/\n\s+perl .*' src-tauri\/Cargo\.toml\n/);
  });

  it('requires tag, package, Tauri, service and app Cargo versions to align', () => {
    expect(() => assertVersionAlignment({
      tagName: `v${version}`,
      packageVersion: version,
      tauriVersion: version,
      serviceCargoVersion: version,
      tauriCargoVersion: version
    })).not.toThrow();
    expect(() => assertVersionAlignment({
      tagName: `v${version}`,
      packageVersion: version,
      tauriVersion: '0.1.0-alpha.0',
      serviceCargoVersion: version,
      tauriCargoVersion: version
    })).toThrow(/versions must all match tag/);
    expect(() => assertVersionAlignment({
      tagName: `v${version}`,
      packageVersion: version,
      tauriVersion: version,
      serviceCargoVersion: '0.1.0-alpha.0',
      tauriCargoVersion: version
    })).toThrow(/versions must all match tag/);
  });

  it('requires exactly five standalone and five plugin assets', async () => {
    const directory = await mkdtemp(path.join(tmpdir(), 'athttp-release-assets-'));
    const expected = [
      `AT.HTTP_${version}_linux_aarch64.atpkg`,
      `AT.HTTP_${version}_linux_aarch64.deb`,
      `AT.HTTP_${version}_linux_x86_64.atpkg`,
      `AT.HTTP_${version}_linux_x86_64.deb`,
      `AT.HTTP_${version}_macos_aarch64.atpkg`,
      `AT.HTTP_${version}_macos_aarch64.dmg`,
      `AT.HTTP_${version}_macos_x86_64.atpkg`,
      `AT.HTTP_${version}_macos_x86_64.dmg`,
      `AT.HTTP_${version}_windows_x86_64.atpkg`,
      `AT.HTTP_${version}_windows_x86_64.exe`
    ];

    expect(expectedReleaseAssets(version)).toEqual(expected);
    try {
      await Promise.all(expected.map((name) => writeFile(path.join(directory, name), 'asset')));
      await expect(verifyReleaseAssets(directory, version)).resolves.toBeUndefined();

      await rm(path.join(directory, expected[0]));
      await expect(verifyReleaseAssets(directory, version))
        .rejects.toThrow(/exact release asset set/);
    } finally {
      await rm(directory, { recursive: true, force: true });
    }
  });
});
