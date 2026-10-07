/**
 * A key-value store in the browser's IndexedDB. In: values browser mode keeps across visits (the
 * VODs folder's handle in vods-folder.ts, the cut-off's labels in cutoff-labels.ts). Out: the same
 * values on a later visit; browser-data-move.ts reads the old ones in this database once.
 */

import { Service } from '@angular/core';

/** The IndexedDB database's name. */
const DB = 'aimview';
/** The one object store in it, keyed by the callers' own keys. */
const STORE = 'kv';

/** A value to store, with its key. */
export type StoreEntry = [key: string, value: unknown];

/**
 * Values kept in this browser across visits (IndexedDB): what localStorage cannot hold, such as a
 * folder's handle or binary labels. Where there is no IndexedDB (tests), nothing is kept.
 */
@Service()
export class BrowserStore {
  /** The database once asked for (null inside: no IndexedDB, or it failed to open). */
  private db: Promise<IDBDatabase | null> | null = null;

  /** Opens the database the first time, creating its store; null where it cannot be opened. */
  private open(): Promise<IDBDatabase | null> {
    this.db ??= new Promise((resolve) => {
      if (typeof indexedDB === 'undefined') return resolve(null);
      const req = indexedDB.open(DB, 1);
      req.onupgradeneeded = () => req.result.createObjectStore(STORE);
      req.onsuccess = () => resolve(req.result);
      req.onerror = () => resolve(null);
    });
    return this.db;
  }

  /** The value kept under the key; undefined when there is none, or no database. */
  async get<T>(key: string): Promise<T | undefined> {
    const db = await this.open();
    if (!db) return undefined;
    return new Promise((resolve) => {
      const req = db.transaction(STORE).objectStore(STORE).get(key);
      req.onsuccess = () => resolve(req.result as T | undefined);
      req.onerror = () => resolve(undefined);
    });
  }

  /** Keeps the value under the key; rejects when the write fails, skips it without a database. */
  async set(key: string, value: unknown): Promise<void> {
    const db = await this.open();
    if (!db) return;
    return new Promise((resolve, reject) => {
      const tx = db.transaction(STORE, 'readwrite');
      tx.objectStore(STORE).put(value, key);
      tx.oncomplete = () => resolve();
      tx.onerror = () => reject(tx.error);
    });
  }

  /** Keeps several values in one transaction: all of them, or none when it fails. */
  async setMany(entries: readonly StoreEntry[]): Promise<void> {
    const db = await this.open();
    if (!db) return;
    return new Promise((resolve, reject) => {
      const tx = db.transaction(STORE, 'readwrite');
      const store = tx.objectStore(STORE);
      for (const [key, value] of entries) store.put(value, key);
      tx.oncomplete = () => resolve();
      tx.onerror = () => reject(tx.error);
    });
  }

  /** Forgets the value under the key. */
  async remove(key: string): Promise<void> {
    const db = await this.open();
    if (!db) return;
    return new Promise((resolve, reject) => {
      const tx = db.transaction(STORE, 'readwrite');
      tx.objectStore(STORE).delete(key);
      tx.oncomplete = () => resolve();
      tx.onerror = () => reject(tx.error);
    });
  }
}
