/**
 * The floating video's bar of buttons.
 *
 * In: the user's clicks. Out: two events the player (player.html) passes to FloatingPlayer.
 */

import { Component, output } from '@angular/core';

/**
 * The floating video's two buttons (floating-player.ts): back to the player, and close (the video
 * stops).
 */
@Component({
  selector: 'app-mini-bar',
  templateUrl: './mini-bar.html',
  styleUrl: './mini-bar.scss',
})
export class MiniBar {
  /** Fires on "Back to the player": the page scrolls back to the player. */
  readonly toPlayer = output();
  /** Fires on close: the video stops and goes back into the player. */
  readonly dismiss = output();
}
