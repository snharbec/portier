import { goto } from '$app/navigation';
import { fetchAhead, offlineUser } from './offline.svelte.ts';
import type { SwipeAction } from './swipe.ts';
import { api, setStoredHandler, setUnauthorizedHandler, type Account, type Category, type Counts, type SavedSearch, type User } from './api.ts';

/** Session-wide state. `tick` changes whenever the server reports new or changed mail. */
export const app = $state({
	ready: false,
	user: null as User | null,
	setupNeeded: false,
	openRegistration: false,
	/** Whether the server can turn Office documents into previews (LibreOffice installed). */
	officePreviews: false,
	counts: {
		screener: 0,
		unread_important: 0,
		unread_flagged: 0,
		unread_feed: 0,
		unread_junk: 0,
		unread_delayed: 0,
		unread_archive: 0,
		unread_trash: 0,
		delayed: 0,
		drafts: 0
	} as Counts,
	accounts: [] as Account[],
	/** Searches saved under a name, shown in the side bar. */
	searches: [] as SavedSearch[],
	/** What sliding a mail left or right does; set in Settings. */
	swipe: { left: ['trash'], right: ['read'] } as { left: SwipeAction[]; right: SwipeAction[] },
	/** Weeks after which read Home conversations are archived automatically; 0 is off. */
	autoArchiveWeeks: 0,
	/** The split view the reader chose: the opened mail beside the mail list, or below it. */
	split: 'off' as 'off' | 'beside' | 'below',
	/** Split view is chosen and possible right now (on a mail list). */
	splitActive: false,
	/** Set while the app shows mail kept on this device because the server cannot be reached: when it was kept. */
	storedSince: null as number | null,
	/** Short message about the last action, shown for a few seconds. */
	notice: '',
	/** Token that takes back the action the notice is about, while that is possible. */
	undo: '',
	tick: 0
});

let events: EventSource | null = null;
let refreshTimer: ReturnType<typeof setTimeout> | undefined;

setStoredHandler((since) => {
	// The oldest of what is shown is what the reader should know about.
	app.storedSince = since === null ? null : Math.min(since, app.storedSince ?? since);
});

setUnauthorizedHandler(() => {
	offlineUser(null);
	app.user = null;
	events?.close();
	events = null;
});

export async function loadSession() {
	const me = await api.get<{
		user: User | null;
		setup_needed: boolean;
		open_registration: boolean;
		office_previews: boolean;
	}>('/me');
	app.user = me.user;
	app.setupNeeded = me.setup_needed;
	app.openRegistration = me.open_registration;
	app.officePreviews = me.office_previews;
	app.ready = true;
	// Mail kept on this device is this user's, or nobody's.
	await offlineUser(app.user?.id ?? null);
	if (app.user) {
		await Promise.all([refreshCounts(), refreshAccounts(), refreshSettings()]);
		connectEvents();
		fetchAhead();
	}
}

/** Everything the side bar counts: unread mail, waiting senders, drafts and saved searches. */
export async function refreshCounts() {
	const [counts, searches] = await Promise.all([api.get<Counts>('/counts'), api.get<SavedSearch[]>('/searches')]);
	app.counts = counts;
	app.searches = searches;
}

export async function refreshSettings() {
	const settings = await api.get<{
		swipe_left: SwipeAction[];
		swipe_right: SwipeAction[];
		auto_archive_weeks: number;
	}>('/settings');
	app.swipe = { left: settings.swipe_left, right: settings.swipe_right };
	app.autoArchiveWeeks = settings.auto_archive_weeks;
}

let noticeTimer: ReturnType<typeof setTimeout> | undefined;

/** How long an action can be undone; the server keeps it a little longer. */
const UNDO_MS = 4000;

/**
 * Says what happened, at the bottom of the window. With `undo`, the token an action came back
 * with, the notice offers to take the action back for as long as that is possible.
 */
export function notify(text: string, undo = '') {
	app.notice = text;
	app.undo = undo;
	clearTimeout(noticeTimer);
	noticeTimer = setTimeout(
		() => {
			app.notice = '';
			app.undo = '';
		},
		undo ? UNDO_MS : 5000
	);
}

/** Takes back the action the notice is about. */
export async function undoLast() {
	const token = app.undo;
	if (!token) return;
	app.undo = '';
	try {
		const undone = await api.post<{ draft: number | null }>(`/undo/${token}`);
		notify('Undone');
		app.tick += 1;
		refreshCounts().catch(() => {});
		// A mail that was about to be sent is a draft again: back to writing it.
		if (undone.draft !== null) goto(`/compose/${undone.draft}`);
	} catch (e) {
		notify((e as Error).message);
	}
}

export async function refreshAccounts() {
	app.accounts = await api.get<Account[]>('/accounts');
}

function connectEvents() {
	events?.close();
	events = new EventSource('/api/events');
	events.onmessage = (event) => {
		if (event.data === 'send_failed') {
			notify('A mail could not be sent. It is back in Drafts, with the reason.');
		}
		// A sync batch sends many events; fold them into one reload.
		clearTimeout(refreshTimer);
		refreshTimer = setTimeout(() => {
			app.tick += 1;
			refreshCounts().catch(() => {});
			fetchAhead();
		}, 400);
	};
}

export async function logout() {
	await api.post('/logout');
	await offlineUser(null);
	events?.close();
	events = null;
	app.user = null;
}

export async function classify(senderId: number, category: Category | null) {
	await api.post(`/senders/${senderId}/category`, { category });
	app.tick += 1;
	await refreshCounts();
}

export const categoryNames: Record<Category, string> = {
	important: 'Home',
	feed: 'Nice to know',
	junk: 'Junk'
};

/** Starts a draft and returns the composer path for it. */
export async function startDraft(kind: string, sourceMessage?: number, to?: string): Promise<string> {
	const { id } = await api.post<{ id: number }>('/drafts', { kind, source_message: sourceMessage, to: to ?? '' });
	rememberDraftReturn(id);
	return `/compose/${id}`;
}

/** Notes the page a draft is opened from, for `draftReturnPath`. */
export function rememberDraftReturn(id: number) {
	try {
		sessionStorage.setItem(RETURN_KEY + id, location.pathname + location.search);
	} catch {
		// No storage: the composer leaves to the Drafts list instead.
	}
}

const RETURN_KEY = 'emscreen.compose.from.';

/**
 * The page a draft was started on, to go back to once it is sent or discarded. Named outright
 * instead of stepping back in the browser's history: the frames that show mail bodies can leave
 * entries of their own there, and stepping back then ends up somewhere else.
 */
export function draftReturnPath(id: string | undefined): string {
	try {
		const from = sessionStorage.getItem(RETURN_KEY + id);
		sessionStorage.removeItem(RETURN_KEY + id);
		if (from?.startsWith('/') && !from.startsWith('/compose')) return from;
	} catch {
		// See rememberDraftReturn().
	}
	return '/drafts';
}
