/**
 * The parts of the File System Access API that Chromium has and TypeScript's DOM types lack: the
 * folder picker, and asking for leave to read a remembered folder again. Global types, which
 * browser mode's VODs folder uses (modes/service/: vods-folder.ts, service.worker.ts).
 */

/** What the page may do with a folder or file: read it, or read and write it. */
type FileSystemPermissionMode = 'read' | 'readwrite';

/** Which leave a permission check or request is about. */
interface FileSystemHandlePermissionDescriptor {
  /** Read, or read and write; read when left out. */
  mode?: FileSystemPermissionMode;
}

/** A folder or file the page holds, with the permission calls Chromium adds. */
interface FileSystemHandle {
  /** Whether the page still has leave to use it, without asking the user. */
  queryPermission(descriptor?: FileSystemHandlePermissionDescriptor): Promise<PermissionState>;
  /** Asks the user for leave to use it again; needs a click or key press first. */
  requestPermission(descriptor?: FileSystemHandlePermissionDescriptor): Promise<PermissionState>;
}

/** The folder picker's options. */
interface DirectoryPickerOptions {
  /** A name under which the browser remembers the folder last picked, to start there next time. */
  id?: string;
  /** The leave asked for with the folder. */
  mode?: FileSystemPermissionMode;
}

/** A kind of file the save dialog offers: its description and its types and extensions. */
interface SaveFileType {
  /** What the files are, in words. */
  description: string;
  /** Each media type and the extensions that go with it. */
  accept: Record<string, string[]>;
}

/** The save dialog's options: the name it suggests and the kinds of file it offers. */
interface SaveFilePickerOptions {
  /** The file name the dialog suggests. */
  suggestedName?: string;
  /** The kinds of file it offers. */
  types?: SaveFileType[];
}

/** The page's window, with Chromium's folder picker and save dialog. */
interface Window {
  /** Asks the user to pick a folder; missing in browsers without the picker. */
  showDirectoryPicker?(options?: DirectoryPickerOptions): Promise<FileSystemDirectoryHandle>;
  /** Asks the user where to save a file; missing in browsers without the dialog. */
  showSaveFilePicker?(options?: SaveFilePickerOptions): Promise<FileSystemFileHandle>;
}
