/**
 * Browser mode's `CropSets`. In: a check folder the user chooses (its files), and the service in
 * the page's crop routes. Out: the folder copied into the data folder's crops/, then the same
 * pages, crops and answers as the review server gives, for the Crops page.
 */

import { inject, Service } from '@angular/core';
import { ServerCropSets } from '../http/server-crop-sets';
import { MountedFiles } from './mounted-files';
import { ChosenFile } from './service-messages';

/**
 * Where browser mode keeps the check folders the user adds: the data folder's crops/ (the app's
 * layout).
 */
const CROPS = '/data/crops';
/** A check folder's name keeps to these characters (service/src/crops.rs `plain_name`). */
const PLAIN_NAME = /^[A-Za-z0-9_-][A-Za-z0-9_.-]*$/;
/** The lists of a check folder the page copies, by their paths in it. */
const LISTS = new Set(['crops.json', 'sets.json']);
/** A crop's picture in a check folder: crops/<name>.png. */
const PICTURE = /^crops\/[^/]+\.png$/i;

/**
 * The check folders in this browser: the review service in the page answers the same routes as the
 * review server (service/src/crops.rs), from the data folder's crops/. A folder the user chooses is
 * copied in first, its lists and pictures only (make_page.py's folder also holds the answers a
 * claude.ai page gave, which are left where they are).
 */
@Service()
export class BrowserCropSets extends ServerCropSets {
  /** Copies the chosen folder into the service's data folder. */
  private readonly files = inject(MountedFiles);
  /** Copies a check folder the user chose into this browser; gives its name. */
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
