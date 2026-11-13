import type { HttpCollectionFolder } from './types';

// folder 自身或任意后代是否含有可见 request。搜索过滤时用于收起完全为空的分组枝,
// 避免留下没有内容的分组标题。countRequests 返回某个 folder 直属的可见 request 数量。
export function folderHasVisibleContent(
  folder: HttpCollectionFolder,
  collectionId: string,
  allFolders: HttpCollectionFolder[],
  countRequests: (collectionId: string, folderId: string) => number
): boolean {
  if (countRequests(collectionId, folder.id) > 0) return true;
  return allFolders
    .filter((f) => f.collectionId === collectionId && f.parentId === folder.id)
    .some((child) => folderHasVisibleContent(child, collectionId, allFolders, countRequests));
}

// 统计 folder 直属 + 所有后代 folder 下的 request 总数,用于分组标题旁的计数(如 Apifox)。
export function folderRequestCount(
  folder: HttpCollectionFolder,
  collectionId: string,
  allFolders: HttpCollectionFolder[],
  countRequests: (collectionId: string, folderId: string) => number
): number {
  let total = countRequests(collectionId, folder.id);
  for (const child of allFolders) {
    if (child.collectionId === collectionId && child.parentId === folder.id) {
      total += folderRequestCount(child, collectionId, allFolders, countRequests);
    }
  }
  return total;
}
