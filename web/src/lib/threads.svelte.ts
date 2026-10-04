import { api, type ThreadSummary } from './api.ts';

/** Conversations a list asks for at a time; a list that is longer offers its older ones. */
const PAGE = 300;

/**
 * The conversations of one list, read in pages, newest first, and narrowed to those with
 * unseen mail when the reader asks for that (for the lists that offer it; the choice is kept
 * in this browser, per list).
 */
export function createThreadList(box: string, narrowable = false) {
	const key = `emscreen.unseen-only.${box}`;
	let threads = $state<ThreadSummary[] | null>(null);
	let error = $state('');
	let more = $state(false);
	let busy = $state(false);
	let unseenOnly = $state(narrowable && restore());

	function restore() {
		try {
			return localStorage.getItem(key) === '1';
		} catch {
			return false;
		}
	}

	const address = (limit: number, offset = 0) =>
		`/threads?box=${box}&limit=${limit}&offset=${offset}${unseenOnly ? '&unseen=1' : ''}`;

	/** Loads the list again, as far as it was shown. `open` is the mail open beside it. */
	async function load(open = 0) {
		const before = threads ?? [];
		const limit = Math.max(PAGE, Math.ceil(before.length / PAGE) * PAGE);
		try {
			const list = await api.get<ThreadSummary[]>(address(limit));
			more = list.length >= limit;
			// The mail open beside the list stays in it although reading it made it seen.
			const kept = unseenOnly ? before.findIndex((t) => t.id === open) : -1;
			if (kept >= 0 && !list.some((t) => t.id === open)) {
				list.splice(Math.min(kept, list.length), 0, { ...before[kept], unread: 0 });
			}
			threads = list;
			error = '';
		} catch (e) {
			error = (e as Error).message;
		}
	}

	/** Adds the next page of older conversations. */
	async function older() {
		if (!threads || busy) return;
		busy = true;
		try {
			const page = await api.get<ThreadSummary[]>(address(PAGE, threads.length));
			const known = new Set(threads.map((t) => t.id));
			threads = [...threads, ...page.filter((t) => !known.has(t.id))];
			more = page.length >= PAGE;
		} catch (e) {
			error = (e as Error).message;
		} finally {
			busy = false;
		}
	}

	function toggleUnseenOnly() {
		unseenOnly = !unseenOnly;
		try {
			localStorage.setItem(key, unseenOnly ? '1' : '0');
		} catch {
			// The choice then lasts until the page is reloaded.
		}
		threads = null;
		return load();
	}

	return {
		get threads() {
			return threads;
		},
		get error() {
			return error;
		},
		/** There are older conversations than the ones shown. */
		get more() {
			return more;
		},
		get busy() {
			return busy;
		},
		get unseenOnly() {
			return unseenOnly;
		},
		load,
		older,
		toggleUnseenOnly
	};
}
