/**
 * How the app looks: a colour theme, and whether it is light or dark. Kept per browser.
 * `app.html` applies the stored choice before the first paint; this module changes it later.
 */
const KEY = 'emscreen.look';

export const themes = [
	{ id: 'harbour', name: 'Harbour', hint: 'Blue on cool grey' },
	{ id: 'forest', name: 'Forest', hint: 'Green on soft sage' },
	{ id: 'plum', name: 'Plum', hint: 'Violet on pale lilac' },
	{ id: 'graphite', name: 'Graphite', hint: 'Black and white, no accent colour' }
] as const;
export type Theme = (typeof themes)[number]['id'];

export const modes = [
	{ id: 'auto', name: 'Like the system' },
	{ id: 'light', name: 'Light' },
	{ id: 'dark', name: 'Dark' }
] as const;
export type Mode = (typeof modes)[number]['id'];

function restore(): { theme: Theme; mode: Mode } {
	try {
		const saved = JSON.parse(localStorage.getItem(KEY) ?? '{}');
		return {
			theme: themes.some((t) => t.id === saved.theme) ? saved.theme : 'harbour',
			mode: modes.some((m) => m.id === saved.mode) ? saved.mode : 'auto'
		};
	} catch {
		// No storage: the default look, following the system.
		return { theme: 'harbour', mode: 'auto' };
	}
}

const systemDark = typeof matchMedia === 'function' ? matchMedia('(prefers-color-scheme: dark)') : null;

/** The choice, and `dark` for what it comes to right now. */
export const look = $state({ ...restore(), dark: false });

function apply() {
	look.dark = look.mode === 'dark' || (look.mode === 'auto' && !!systemDark?.matches);
	if (typeof document === 'undefined') return;
	document.documentElement.dataset.theme = look.theme;
	document.documentElement.dataset.mode = look.dark ? 'dark' : 'light';
	// An installed app's title bar and a phone's status bar take the page's own colour.
	const paper = getComputedStyle(document.documentElement).getPropertyValue('--paper').trim();
	if (paper) document.querySelector('meta[name="theme-color"]')?.setAttribute('content', paper);
}

export function setLook(change: { theme?: Theme; mode?: Mode }) {
	Object.assign(look, change);
	apply();
	try {
		localStorage.setItem(KEY, JSON.stringify({ theme: look.theme, mode: look.mode }));
	} catch {
		// The choice then lasts until the page is reloaded.
	}
}

apply();
// "Like the system" follows the system when it switches between day and night.
systemDark?.addEventListener('change', apply);
