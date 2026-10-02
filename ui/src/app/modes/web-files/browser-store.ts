import { Injectable } from '@angular/core';

const DB = 'aimview';
const STORE = 'kv';

/** A value to store, with its key. */
export type StoreEntry = [key: string, value: unknown];

/**
 * Values kept in this browser across visits (IndexedDB): what localStorage cannot hold, such as a folder's handle or
 * a copy of the stats files. Where there is no IndexedDB (tests), nothing is kept.
 */
@Injectable({ providedIn: 'root' })
export class BrowserStore {
  private db: Promise<IDBDatabase | null> | null = null;

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

  async get<T>(key: string): Promise<T | undefined> {
    const db = await this.open();
    if (!db) return undefined;
    return new Promise((resolve) => {
      const req = db.transaction(STORE).objectStore(STORE).get(key);
      req.onsuccess = () => resolve(req.result as T | undefined);
      req.onerror = () => resolve(undefined);
    });
  }

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

  /** Several values in one transaction. */
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
