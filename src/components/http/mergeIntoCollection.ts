import type { HttpCollection, HttpCollectionFolder, HttpCollectionRequest, HttpRequestSpec } from './types';
import type { ImportedOpenApiCollection, ImportedRequest, ImportedFolder } from './openapiImport';

export interface MergeInput {
  collection: HttpCollection;
  folders: HttpCollectionFolder[];
  requests: HttpCollectionRequest[];
}

export interface MergeResult {
  folders: HttpCollectionFolder[];
  requests: HttpCollectionRequest[];
  deletedFolderIds: string[];
  deletedRequestIds: string[];
  diff: { added: number; updated: number; deleted: number };
}

function mergeSpec(
  existing: HttpRequestSpec,
  incoming: HttpRequestSpec
): HttpRequestSpec {
  return {
    ...incoming,
    timeoutSeconds: existing.timeoutSeconds,
    followRedirects: existing.followRedirects,
    verifySsl: existing.verifySsl,
    saveToHistory: existing.saveToHistory
  };
}

export function mergeIntoCollection(
  existing: MergeInput,
  incoming: ImportedOpenApiCollection
): MergeResult {
  const now = Date.now();
  const collectionId = existing.collection.id;

  const incomingByKey = new Map<string, ImportedRequest>();
  for (const r of incoming.requests) {
    if (r.sourceKey) incomingByKey.set(r.sourceKey, r);
  }

  const userAdded = existing.requests.filter((r) => !r.sourceKey);
  const managedExisting = existing.requests.filter((r) => !!r.sourceKey);

  const folderByName = new Map<string, HttpCollectionFolder>();
  for (const f of existing.folders) folderByName.set(f.name, f);

  const nextFolders: HttpCollectionFolder[] = [];
  const seenFolderIds = new Set<string>();
  for (const inFolder of incoming.folders) {
    const existingFolder = folderByName.get(inFolder.name);
    if (existingFolder) {
      nextFolders.push(existingFolder);
      seenFolderIds.add(existingFolder.id);
    } else {
      const created: HttpCollectionFolder = {
        id: inFolder.id,
        collectionId,
        parentId: inFolder.parentId,
        name: inFolder.name,
        orderIndex: nextFolders.length,
        updatedAt: now
      };
      nextFolders.push(created);
      folderByName.set(created.name, created);
      seenFolderIds.add(created.id);
    }
  }
  for (const f of existing.folders) {
    if (!seenFolderIds.has(f.id)) {
      nextFolders.push(f);
    }
  }

  const nextRequests: HttpCollectionRequest[] = [];
  const deletedRequestIds: string[] = [];
  let added = 0;
  let updated = 0;

  for (const r of userAdded) nextRequests.push(r);

  for (const r of managedExisting) {
    const inc = incomingByKey.get(r.sourceKey!);
    if (inc) {
      nextRequests.push({
        ...r,
        name: inc.name,
        method: inc.method,
        spec: mergeSpec(r.spec, inc.spec),
        updatedAt: now
      });
      updated += 1;
    } else {
      deletedRequestIds.push(r.id);
    }
  }

  const handled = new Set(
    managedExisting.filter((r) => incomingByKey.has(r.sourceKey!)).map((r) => r.sourceKey!)
  );

  for (const inc of incoming.requests) {
    if (!inc.sourceKey) continue;
    if (handled.has(inc.sourceKey)) continue;
    const targetFolder = resolveTargetFolderId(inc, incoming.folders, nextFolders);
    nextRequests.push({
      id: inc.id,
      collectionId,
      folderId: targetFolder,
      name: inc.name,
      method: inc.method,
      spec: inc.spec,
      orderIndex: nextRequests.length,
      updatedAt: now,
      sourceKey: inc.sourceKey
    });
    added += 1;
  }

  const activeRequestFolderIds = new Set(
    nextRequests.map((r) => r.folderId).filter((id): id is string => !!id)
  );
  const deletedFolderIds: string[] = [];
  const finalFolders = nextFolders.filter((f) => {
    if (activeRequestFolderIds.has(f.id)) return true;
    deletedFolderIds.push(f.id);
    return false;
  });

  return {
    folders: finalFolders,
    requests: nextRequests,
    deletedFolderIds,
    deletedRequestIds,
    diff: { added, updated, deleted: deletedRequestIds.length }
  };
}

function resolveTargetFolderId(
  inc: ImportedRequest,
  incomingFolders: ImportedFolder[],
  nextFolders: HttpCollectionFolder[]
): string | null {
  if (!inc.folderId) return null;
  const incFolder = incomingFolders.find((f) => f.id === inc.folderId);
  if (!incFolder) return null;
  const match = nextFolders.find((f) => f.name === incFolder.name);
  return match?.id ?? null;
}
