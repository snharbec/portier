import { SvelteSet } from 'svelte/reactivity';

/**
 * Ids ticked in a list. `visible` narrows them to what the list still shows.
 * The checkboxes only show while selecting is `active`: between `start` and `stop`.
 */
export function createSelection() {
	const ids = new SvelteSet<number>();
	let active = $state(false);
	return {
		get active() {
			return active;
		},
		start: () => void (active = true),
		/** Ends selecting and forgets what was ticked. */
		stop: () => {
			active = false;
			ids.clear();
		},
		has: (id: number) => ids.has(id),
		toggle: (id: number) => void (ids.has(id) ? ids.delete(id) : ids.add(id)),
		set: (all: number[]) => {
			ids.clear();
			for (const id of all) ids.add(id);
		},
		clear: () => ids.clear(),
		all: () => [...ids],
		visible: <T,>(items: T[], idOf: (item: T) => number) => items.filter((item) => ids.has(idOf(item)))
	};
}

export type Selection = ReturnType<typeof createSelection>;
