import { inject, Service } from '@angular/core';
import { ServerCropSets } from '../http/server-crop-sets';
import { MountedFiles } from './mounted-files';
import { ChosenFile } from './service-messages';

/** Where browser mode keeps the check folders the user adds: the data folder's crops/ (the app's layout). */
const CROPS = '/data/crops';
/** A check folder's name keeps to these characters (service/src/crops.rs `plain_name`). */
const PLAIN_NAME = /^[A-Za-z0-9_-][A-Za-z0-9_.-]*$/;
/** What of a check folder the page reads: its lists, and its pictures. */
const LISTS = new Set(['crops.json', 'sets.json']);
const PICTURE = /^crops\/[^/]+\.png$/i;

/**
 * The check folders in this browser: the review service in the page answers the same routes as the review server
 * (service/src/crops.rs), from the data folder's crops/. A folder the user chooses is copied in first, its lists and
 * pictures only (make_page.py's folder also holds the answers a claude.ai page gave, which are left where they are).
 */
@Service()
export class BrowserCropSets extends ServerCropSets {
  private readonly files = inject(MountedFiles);
  override readonly addFolder = (files: readonly File[]): Promise<string> => this.copyFolder(files);

  /** Copies a chosen check folder into crops/<its name>; rejects a folder that is not one. */
  private async copyFolder(files: readonly File[]): Promise<string> {
    const chosen: ChosenFile[] = [];
    let name = '';
    for (const file of files) {
      const [top, ...rest] = (file.webkitRelativePath || file.name).split('/');
      const path = rest.join('/');
      if (!LISTS.has(path) && !PICTURE.test(path)) continue;
      name ||= top;
      if (top === name) chosen.push({ path, file });
    }
    if (!chosen.some((file) => file.path === 'crops.json'))
      throw new Error(
        'That folder has no crops.json: choose a check folder that make_page.py wrote',
      );
    if (!PLAIN_NAME.test(name))
      throw new Error(`A check folder's name keeps to letters, digits, _ - and . (not ${name})`);
    await this.files.copyIn(`${CROPS}/${name}`, chosen, () => undefined);
    return name;
  }
}
