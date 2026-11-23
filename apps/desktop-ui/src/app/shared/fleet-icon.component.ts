import { Component, input } from '@angular/core';

@Component({
  selector: 'fleet-icon',
  standalone: true,
  template: `
    <svg
      [attr.width]="size()"
      [attr.height]="size()"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      stroke-width="1.9"
      stroke-linecap="round"
      stroke-linejoin="round"
      class="shrink-0"
      aria-hidden="true"
    >
      @switch (name()) {
        @case ('radar') {
          <circle cx="12" cy="12" r="9" />
          <circle cx="12" cy="12" r="5" />
          <path d="M12 12 18.5 5.5" />
          <circle cx="12" cy="12" r="1.4" fill="currentColor" stroke="none" />
        }
        @case ('workflow') {
          <rect x="3" y="4" width="6" height="6" rx="1.5" />
          <rect x="15" y="4" width="6" height="6" rx="1.5" />
          <rect x="15" y="14" width="6" height="6" rx="1.5" />
          <path d="M9 7h6M12 7v10h3" />
        }
        @case ('tree') {
          <circle cx="6" cy="6" r="2.5" />
          <circle cx="18" cy="10" r="2.5" />
          <circle cx="18" cy="18" r="2.5" />
          <path d="M6 8.5V18h9.5M6 10h9.5" />
        }
        @case ('route') {
          <circle cx="5" cy="18" r="2.5" />
          <circle cx="19" cy="6" r="2.5" />
          <path d="M7.5 18H12a3 3 0 0 0 3-3V9a3 3 0 0 1 1.5-2.6" />
          <path d="M16 4l3 2-3 2" />
        }
        @case ('inbox') {
          <path d="M4 13V6a2 2 0 0 1 2-2h12a2 2 0 0 1 2 2v7" />
          <path d="M3 13h5l2 3h4l2-3h5v5a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-5Z" />
        }
        @case ('server') {
          <rect x="3" y="4" width="18" height="6.5" rx="1.5" />
          <rect x="3" y="13.5" width="18" height="6.5" rx="1.5" />
          <circle cx="7" cy="7.25" r="0.9" fill="currentColor" stroke="none" />
          <circle cx="7" cy="16.75" r="0.9" fill="currentColor" stroke="none" />
          <path d="M11 7.25h6M11 16.75h6" />
        }
        @case ('sparkles') {
          <path d="M12 3l1.9 5.1L19 10l-5.1 1.9L12 17l-1.9-5.1L5 10l5.1-1.9L12 3Z" />
          <path d="M19 15l.9 2.1L22 18l-2.1.9L19 21l-.9-2.1L16 18l2.1-.9L19 15Z" />
        }
        @case ('shield') {
          <path d="M12 3l7 3v5c0 4.5-3 8.2-7 10-4-1.8-7-5.5-7-10V6l7-3Z" />
          <path d="M9.5 12l1.8 1.8L15 10" />
        }
        @case ('terminal') {
          <rect x="3" y="4" width="18" height="16" rx="2" />
          <path d="M7 9l3 3-3 3M12.5 15H17" />
        }
        @case ('git-branch') {
          <circle cx="6" cy="6" r="2.2" />
          <circle cx="18" cy="6" r="2.2" />
          <circle cx="6" cy="18" r="2.2" />
          <path d="M6 8.2v7.6M18 8.2a6 6 0 0 1-6 6H6" />
        }
        @case ('bolt') {
          <path d="M13 2L4.5 13H12l-1 9 8.5-11H12l1-9Z" />
        }
        @case ('plus') {
          <path d="M12 5v14M5 12h14" />
        }
        @case ('refresh') {
          <path d="M20 11a8 8 0 0 0-14.5-4.5M4 4v4h4" />
          <path d="M4 13a8 8 0 0 0 14.5 4.5M20 20v-4h-4" />
        }
        @case ('copy') {
          <rect x="9" y="9" width="11" height="11" rx="2" />
          <path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1" />
        }
        @case ('play') {
          <polygon points="6,4 20,12 6,20" fill="currentColor" stroke="none" />
        }
        @case ('stop') {
          <rect x="6" y="6" width="12" height="12" rx="2" fill="currentColor" stroke="none" />
        }
        @case ('arrow-right') {
          <path d="M5 12h14M13 6l6 6-6 6" />
        }
        @case ('sliders') {
          <path d="M4 6h16M4 12h16M4 18h16" />
          <circle cx="9" cy="6" r="2" fill="var(--surface-1)" />
          <circle cx="15" cy="12" r="2" fill="var(--surface-1)" />
          <circle cx="11" cy="18" r="2" fill="var(--surface-1)" />
        }
        @case ('alert') {
          <path d="M12 3L2.5 20h19L12 3Z" />
          <path d="M12 9v5M12 17.5h.01" />
        }
        @case ('check') {
          <path d="M5 12.5l4.5 4.5L19 7.5" />
        }
        @case ('chart') {
          <path d="M4 19h16" />
          <rect x="6" y="11" width="3" height="6" rx="1" />
          <rect x="11" y="7" width="3" height="10" rx="1" />
          <rect x="16" y="4" width="3" height="13" rx="1" />
        }
        @case ('bell') {
          <path d="M6 9a6 6 0 0 1 12 0c0 4.5 2 6 2 6H4s2-1.5 2-6Z" />
          <path d="M10 19a2 2 0 0 0 4 0" />
        }
        @case ('search') {
          <circle cx="11" cy="11" r="7" />
          <path d="m20 20-3.5-3.5" />
        }
        @case ('send') {
          <path d="M22 2 11 13" />
          <path d="M22 2 15 22l-4-9-9-4 20-7Z" />
        }
        @case ('close') {
          <path d="M18 6 6 18M6 6l12 12" />
        }
        @case ('command') {
          <path d="M18 3a3 3 0 0 0-3 3v12a3 3 0 0 0 3 3 3 3 0 0 0 3-3 3 3 0 0 0-3-3H6a3 3 0 0 0-3 3 3 3 0 0 0 3 3 3 3 0 0 0 3-3V6a3 3 0 0 0-3-3 3 3 0 0 0-3 3 3 3 0 0 0 3 3h12a3 3 0 0 0 3-3 3 3 0 0 0-3-3Z" />
        }
        @default {
          <circle cx="12" cy="12" r="7" />
        }
      }
    </svg>
  `,
})
export class FleetIconComponent {
  readonly name = input.required<string>();
  readonly size = input<number>(16);
}
