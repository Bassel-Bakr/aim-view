/// <reference lib="webworker" />
/**
 * KovaaK's files an earlier version copied into this browser (packs in aimview/kovaak-packs, and
 * before them a file each in aimview/kovaak), moved once into the review service's database as the
 * user's choosing sends them (kovaak-batch.ts), then removed: the service keeps only each stats
 * file's run and each scenario's facts. In: the old copies. Out: the batches the service is sent,
 * and the copies removed once they are all sent.
 */
import { freshFiles, KovaakKept, sendBatches } from './kovaak-batch';
import { DirMount, KovaakMount, PackStore } from './mounts';
import { ChosenFile, ServiceAnswer, ServiceMethod } from './service-messages';

/** The app's folder in the private file system. */
const APP = 'aimview';
/** The old copies' folders in it: the packs, and the copies a file each. */
const OLD_FOLDERS = ['kovaak-packs', 'kovaak'];
/** A request answered: its status. */
const OK = 200;

/** A request to the service, in the worker's turn. */
export type ServiceSend = (
  method: ServiceMethod,
  path: string,
  body: Uint8Array,
) => Promise<ServiceAnswer>;

/** A folder of the app's, when it is there. */
async function folder(app: FileSystemDirectoryHandle, name: string): Promise<boolean> {
  return app.getDirectoryHandle(name).then(
    () => true,
    () => false,
  );
}

/** Every file of the old copies at its /kovaak path: stats/, scenarios/, workshop/<item>/. */
async function oldFiles(old: KovaakMount): Promise<ChosenFile[]> {
  const out: ChosenFile[] = [];
  const list = (names: string[]) => old.list(names, 0).catch(() => []);
  const add = async (names: string[]) => {
    for (const [name, isDir] of await list(names)) {
      if (!isDir)
        out.push({ path: [...names, name].join('/'), file: await old.file([...names, name]) });
    }
  };
  await add(['stats']);
  await add(['scenarios']);
  for (const [item, isDir] of await list(['workshop'])) if (isDir) await add(['workshop', item]);
  return out;
}

/**
 * Sends the old copies' files the service does not keep yet, then removes the copies. Does
 * nothing when there are none; a failure leaves them for the next start.
 */
export async function moveKovaakCopies(send: ServiceSend): Promise<void> {
  const app = await (await navigator.storage.getDirectory()).getDirectoryHandle(APP);
  const there = await Promise.all(OLD_FOLDERS.map((name) => folder(app, name)));
  if (!there.some(Boolean)) return;
  const [packs, older] = OLD_FOLDERS.map((name) => app.getDirectoryHandle(name, { create: true }));
  const old = new KovaakMount(new PackStore(packs), new DirMount(older, false));
  const asked = await send('GET', '/api/kovaak_files', new Uint8Array());
  if (asked.status !== OK)
    throw new Error(`The service did not say what it keeps (${asked.status})`);
  const kept = JSON.parse(new TextDecoder().decode(asked.body)) as KovaakKept;
  const post = async (body: Uint8Array) => {
    const answer = await send('POST', '/api/kovaak_files', body);
    if (answer.status !== OK) throw new Error(`A batch was refused (${answer.status})`);
  };
  await sendBatches(freshFiles(await oldFiles(old), kept), post, () => undefined);
  for (const name of OLD_FOLDERS)
    await app.removeEntry(name, { recursive: true }).catch(() => undefined);
}
