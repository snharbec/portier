import { SvelteSet } from 'svelte/reactivity';

/** Ids ticked in a list. `visible` narrows them to what the list still shows. */
export function createSelection() {
	const ids = new SvelteSet<number>();
	return {
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
