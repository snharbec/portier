import type { SearchHit } from './api.ts';

const KEY = 'emscreen.search';

function restore(): { query: string; hits: SearchHit[] | null } {
	try {
		const saved = sessionStorage.getItem(KEY);
		if (saved) return JSON.parse(saved);
	} catch {
		// Storage unavailable or content unreadable: start with an empty search.
	}
	return { query: '', hits: null };
}

/**
 * The last search, kept while moving between its results and the search page.
 * Stored for the tab so a reload inside a result keeps the previous/next buttons.
 */
export const search = $state(restore());

export function rememberSearch() {
	try {
		sessionStorage.setItem(KEY, JSON.stringify(search));
	} catch {
		// Without storage the search still works until the page is reloaded.
	}
}

export const resultPath = (index: number) => `/thread/${search.hits![index].thread_id}?hit=${index}`;
