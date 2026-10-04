/**
 * Sender pictures are cached by the browser for a while. When the user sets or removes a
 * picture of their own, the address gets a new version here, which goes into the picture's
 * web address so the change shows at once. Kept per browser.
 */
const KEY = 'emscreen.pictures';

function restore(): Record<string, number> {
	try {
		return JSON.parse(localStorage.getItem(KEY) ?? '{}') ?? {};
	} catch {
		return {};
	}
}

export const pictures = $state({ version: restore() });

export function pictureChanged(address: string) {
	pictures.version[address.toLowerCase()] = Date.now();
	try {
		localStorage.setItem(KEY, JSON.stringify(pictures.version));
	} catch {
		// The new picture then shows until the page is reloaded, and again once the cache runs out.
	}
}
