import { Component, signal } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import { Badge, BadgeTone } from './badge';

@Component({
  imports: [Badge],
  template: `<span appBadge [tone]="tone()">reviewed</span>`,
})
class Host {
  readonly tone = signal<BadgeTone>('neutral');
}

describe('Badge', () => {
  it('gives the element its class and its tone as a data attribute', async () => {
    const fixture = TestBed.createComponent(Host);
    await fixture.whenStable();
    const el: HTMLElement = fixture.nativeElement.querySelector('span');
    expect(el.classList).toContain('badge');
    expect(el.dataset['tone']).toBe('neutral');
    fixture.componentInstance.tone.set('good');
    await fixture.whenStable();
    expect(el.dataset['tone']).toBe('good');
  });
});
