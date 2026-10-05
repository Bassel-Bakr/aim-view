import { Component, output } from '@angular/core';

/** The floating video's two buttons (floating-player.ts): back to the player, and close (the video stops). */
@Component({
  selector: 'app-mini-bar',
  templateUrl: './mini-bar.html',
  styleUrl: './mini-bar.scss',
})
export class MiniBar {
  readonly toPlayer = output();
  readonly dismiss = output();
}
