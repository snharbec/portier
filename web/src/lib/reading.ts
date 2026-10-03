/**
 * The list a conversation was opened from: its conversations in order, and the list's address.
 * Lets the email view move on to the next conversation after one is archived or trashed.
 * Kept for the browser tab so it survives a reload.
 */
const KEY = 'emscreen.reading';

interface Reading {
	ids: number[];
	back: string;
}

let reading: Reading = restore();

function restore(): Reading {
	try {
		const saved = sessionStorage.getItem(KEY);
		if (saved) return JSON.parse(saved);
	} catch {
		// No storage: the list is only remembered until the page is reloaded.
	}
	return { ids: [], back: '/' };
}

function remember() {
	try {
		sessionStorage.setItem(KEY, JSON.stringify(reading));
	} catch {
		// See restore().
	}
}

export function startReading(ids: number[], back: string) {
	reading = { ids: [...ids], back };
	remember();
}

/** The conversation before (-1) or after (1) this one in the list it was opened from. */
export function neighbour(id: number, step: -1 | 1): number | undefined {
	const index = reading.ids.indexOf(id);
	return index < 0 ? undefined : reading.ids[index + step];
}

/**
 * Takes a conversation out of the list it was opened from and says where to go:
 * the conversation that followed it, the one before it if it was the last, or back to the list.
 * `null` when the conversation was not opened from a list.
 */
export function afterRemoving(id: number): string | null {
	const index = reading.ids.indexOf(id);
	if (index < 0) return null;
	reading.ids.splice(index, 1);
	remember();
	const next = reading.ids[Math.min(index, reading.ids.length - 1)];
	return next === undefined ? reading.back : `/thread/${next}`;
}
