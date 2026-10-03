import { Component, computed, ElementRef, inject, signal, viewChild } from '@angular/core';
import { form, FormField, pattern, required } from '@angular/forms/signals';
import { errorMessage, LinkFormat, LinkInfo } from '../../api';
import { Button } from '../../controls/button';
import { formatSize } from '../../format';
import { Library } from '../../services/library';

/** The link and the quality chosen, as the form edits them. */
export interface LinkFields {
  url: string;
  format: string;
}

/** What a link offers, and the link it was read for. */
export interface ReadLink {
  url: string;
  info: LinkInfo;
}

/** A quality to pick: the format's id, and how the picker names it. */
export interface QualityChoice {
  id: string;
  label: string;
}

const WEB_LINK = /^https?:\/\/\S+$/i;

/** A quality as "2560x1440 · 60 fps · AV1 · 412 MB", with what is known of it. */
export function qualityLabel(f: LinkFormat): string {
  const frame = f.width && f.height ? `${f.width}x${f.height}` : f.height ? `${f.height}p` : null;
  const size = !f.size ? null : f.size < 1e6 ? 'under 1 MB' : formatSize(f.size);
  const parts = [frame, f.fps ? `${Math.round(f.fps)} fps` : null, f.codec, size];
  return parts.filter((p) => p !== null).join(' · ') || f.id;
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
  private readonly library = inject(Library);
  protected readonly source = this.library.source;
  private readonly dialog = viewChild.required<ElementRef<HTMLDialogElement>>('dialog');
  protected readonly model = signal<LinkFields>({ url: '', format: '' });
  protected readonly fields = form(this.model, (p) => {
    required(p.url);
    pattern(p.url, WEB_LINK);
  });
  /** The server that downloads links for this browser, where the mode has one. */
  protected readonly server = this.source.linkServer && form(this.source.linkServer);
  private readonly read = signal<ReadLink | null>(null);
  /** What the link in the field offers, once it is read. */
  protected readonly info = computed(() => {
    const r = this.read();
    return r && r.url === this.model().url.trim() ? r.info : null;
  });
  /** The qualities to pick from, best first; none when there is only one. */
  protected readonly choices = computed<QualityChoice[]>(() => {
    const formats = this.info()?.formats ?? [];
    return formats.length < 2
      ? []
      : formats.map((f, i) => ({ id: f.id, label: qualityLabel(f) + (i === 0 ? ' (best)' : '') }));
  });
  /** What is being done ("Reading the link"), or null. */
  protected readonly busy = signal<string | null>(null);
  protected readonly problem = signal<string | null>(null);

  protected open(): void {
    this.dialog().nativeElement.showModal();
  }

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
      this.model.update((m) => ({ ...m, format: info.formats[0]?.id ?? '' }));
      return info;
    } catch (e) {
      this.problem.set(errorMessage(e));
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
    } catch (e) {
      this.problem.set(errorMessage(e));
    } finally {
      this.busy.set(null);
    }
  }
}
