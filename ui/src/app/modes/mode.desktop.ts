import { Mode } from '../platform/mode';
import { MODE as BROWSER } from './mode.browser';

/**
 * The desktop app (Tauri 2, desktop/). Until it exists, it runs the browser mode's services in its window; then
 * tauri/ takes over, piece by piece: the disk, the review core built natively, and the app's data folder.
 */
export const MODE: Mode = { ...BROWSER, name: 'desktop' };
