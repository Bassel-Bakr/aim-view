import { Component, signal } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import { Button, ButtonIntent } from './button';

@Component({
  imports: [Button],
  template: `<button appButton [intent]="intent()" class="ml-2">Go</button>`,
})
class Host {
  readonly intent = signal<ButtonIntent>('normal');
}

describe('Button', () => {
  it('gives the button its class and its intent as a data attribute, beside the classes it already has', async () => {
    const fixture = TestBed.createComponent(Host);
    await fixture.whenStable();
    const el: HTMLButtonElement = fixture.nativeElement.querySelector('button');
    expect(el.classList).toContain('button');
    expect(el.classList).toContain('ml-2');
    expect(el.dataset['intent']).toBe('normal');
    fixture.componentInstance.intent.set('primary');
    await fixture.whenStable();
    expect(el.dataset['intent']).toBe('primary');
  });
});
