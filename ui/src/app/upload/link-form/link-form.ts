/**
 * Adding a recording from a link (`LinkForm`, the Upload panel's From a link dialog). In: the
 * user's link and the RecordingSource's linkInfo (title and qualities). Out: the recording
 * RecordingSource.addLink lists, which opens while it downloads.
 */

import { Component, computed, ElementRef, inject, signal, viewChild } from '@angular/core';
import { form, FormField, pattern, required } from '@angular/forms/signals';
import { errorMessage, LinkFormat, LinkInfo } from '../../api';
import { Button } from '../../controls/button';
import { formatSize } from '../../format';
import { Library } from '../../services/library';

/** The link and the quality chosen, as the form edits them. */
export interface LinkFields {
  /** The link as typed or pasted. */
  url: string;
  /** The chosen format's id; empty for none. */
  format: string;
}

/** What a link offers, and the link it was read for. */
export interface ReadLink {
  /** The link that was read. */
  url: string;
  /** Its title and qualities. */
  info: LinkInfo;
}

/** A quality to pick: the format's id, and how the picker names it. */
export interface QualityChoice {
  /** The format's id. */
  id: string;
  /** How the picker names it. */
  label: string;
}

/** A web address: http or https, with no spaces. */
const WEB_LINK = /^https?:\/\/\S+$/i;

/** A quality as "2560x1440 · 60 fps · AV1 · 412 MB", with what is known of it. */
export function qualityLabel(format: LinkFormat): string {
  const frame =
    format.width && format.height
      ? `${format.width}x${format.height}`
      : format.height
        ? `${format.height}p`
        : null;
  const size = !format.size ? null : format.size < 1e6 ? 'under 1 MB' : formatSize(format.size);
  const parts = [frame, format.fps ? `${Math.round(format.fps)} fps` : null, format.codec, size];
  return parts.filter((part) => part !== null).join(' · ') || format.id;
}

/**
 * From a link: a video's page on YouTube, Twitch, Medal and the other sites yt-dlp reads, or a video file's address.
 * Pasted, the link is read; where it offers more than one quality, the picker shows them with the best chosen. Add
 * lists the recording and opens it while it downloads (RecordingSource.addLink).
 */
@Component({
  imports: [Button, FormField],
  selector: 'app-link-form',
  templateUrl: './link-form.html',
  styleUrl: './link-form.scss',
})
export class LinkForm {
  /** The recordings; the one added opens. */
  private readonly library = inject(Library);
  /** Where links are read and added. */
  protected readonly source = this.library.source;
  /** The From a link dialog. */
  private readonly dialog = viewChild.required<ElementRef<HTMLDialogElement>>('dialog');
  /** The form's values. */
  protected readonly model = signal<LinkFields>({ url: '', format: '' });
  /** The form: the link is needed and must be a web address. */
  protected readonly fields = form(this.model, (path) => {
    required(path.url);
    pattern(path.url, WEB_LINK);
  });
  /** The server that downloads links for this browser, where the mode has one. */
  protected readonly server = this.source.linkServer && form(this.source.linkServer);
  /** The last link read and what it offers; null before one is read. */
  private readonly read = signal<ReadLink | null>(null);
  /** What the link in the field offers, once it is read. */
  protected readonly info = computed(() => {
    const lastRead = this.read();
    return lastRead && lastRead.url === this.model().url.trim() ? lastRead.info : null;
  });
  /** The qualities to pick from, best first; none when there is only one. */
  protected readonly choices = computed<QualityChoice[]>(() => {
    const formats = this.info()?.formats ?? [];
    return formats.length < 2
      ? []
      : formats.map((format, i) => ({
          id: format.id,
          label: qualityLabel(format) + (i === 0 ? ' (best)' : ''),
        }));
  });
  /** What is being done ("Reading the link"), or null. */
  protected readonly busy = signal<string | null>(null);
  /** Why the link could not be read or added; null when nothing failed. */
  protected readonly problem = signal<string | null>(null);

  /** Opens the dialog. */
  protected open(): void {
    this.dialog().nativeElement.showModal();
  }

  /** Closes the dialog. */
  protected close(): void {
    this.dialog().nativeElement.close();
  }

  /** A pasted link is read at once (the field takes the text after the paste event). */
  protected readPasted(): void {
    setTimeout(() => void this.readLink());
  }

  /** Reads what the link offers, unless it was read already; null when it cannot be read. */
  private async readLink(): Promise<LinkInfo | null> {
    const url = this.model().url.trim();
    if (this.fields.url().invalid()) return null;
    const known = this.info();
    if (known) return known;
    this.busy.set('Reading the link');
    this.problem.set(null);
    try {
      const info = await this.source.linkInfo(url);
      this.read.set({ url, info });
      this.model.update((current) => ({ ...current, format: info.formats[0]?.id ?? '' }));
      return info;
    } catch (error) {
      this.problem.set(errorMessage(error));
      return null;
    } finally {
      this.busy.set(null);
    }
  }

  /**
   * Adds the recording and opens it. A link not read yet is read first: with qualities to pick from, the picker
   * shows, and Add again adds it.
   */
  protected async addLink(event: Event): Promise<void> {
    event.preventDefault();
    if (this.busy()) return;
    const shown = this.info() !== null;
    const info = await this.readLink();
    if (!info || (!shown && this.choices().length)) return;
    const url = this.model().url.trim();
    this.busy.set('Adding it');
    try {
      const id = await this.source.addLink(
        url,
        info.formats.length ? this.model().format || null : null,
      );
      this.library.selectedId.set(id);
      this.model.set({ url: '', format: '' });
      this.read.set(null);
      this.close();
    } catch (error) {
      this.problem.set(errorMessage(error));
    } finally {
      this.busy.set(null);
    }
  }
}
