import { api, type SearchHit } from './api.ts';

const KEY = 'emscreen.search';

interface SearchState {
	/** What the field holds. */
	query: string;
	/** The query `hits` are the answer to; lags behind `query` while typing. */
	answered: string;
	hits: SearchHit[] | null;
	error: string;
}

function restore(): SearchState {
	try {
		const saved = sessionStorage.getItem(KEY);
		if (saved) {
			const { query, answered, hits } = JSON.parse(saved);
			return { query: query ?? '', answered: answered ?? query ?? '', hits: hits ?? null, error: '' };
		}
	} catch {
		// Storage unavailable or content unreadable: start with an empty search.
	}
	return { query: '', answered: '', hits: null, error: '' };
}

/**
 * The last search, shared by the field in the top bar, the results page and the
 * previous/next buttons inside a result. Stored for the tab so a reload keeps it.
 */
export const search = $state(restore());

export function rememberSearch() {
	try {
		sessionStorage.setItem(KEY, JSON.stringify({ query: search.query, answered: search.answered, hits: search.hits }));
	} catch {
		// Without storage the search still works until the page is reloaded.
	}
}

let latest = 0;

/** Runs the query in the field. An answer that arrives after a newer query was sent is dropped. */
export async function runSearch() {
	const mine = ++latest;
	const asked = search.query;
	if (!asked.trim()) {
		search.answered = '';
		search.hits = null;
		search.error = '';
	} else {
		try {
			const hits = await api.get<SearchHit[]>(`/search?q=${encodeURIComponent(asked)}`);
			if (mine !== latest) return;
			search.answered = asked;
			search.hits = hits;
			search.error = '';
		} catch (e) {
			if (mine !== latest) return;
			search.answered = asked;
			search.hits = null;
			search.error = (e as Error).message;
		}
	}
	rememberSearch();
}

export const resultPath = (index: number) => `/thread/${search.hits![index].thread_id}?hit=${index}`;
