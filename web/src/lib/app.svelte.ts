import type { SwipeAction } from './swipe.ts';
import { api, setUnauthorizedHandler, type Account, type Category, type Counts, type SavedSearch, type User } from './api.ts';

/** Session-wide state. `tick` changes whenever the server reports new or changed mail. */
export const app = $state({
	ready: false,
	user: null as User | null,
	setupNeeded: false,
	openRegistration: false,
	/** Whether the server can turn Office documents into previews (LibreOffice installed). */
	officePreviews: false,
	counts: { screener: 0, unread_important: 0, drafts: 0 } as Counts,
	accounts: [] as Account[],
	/** Searches saved under a name, shown in the side bar. */
	searches: [] as SavedSearch[],
	/** What sliding a mail left or right does; set in Settings. */
	swipe: { left: ['trash'], right: ['read'] } as { left: SwipeAction[]; right: SwipeAction[] },
	/** Short message about the last action, shown for a few seconds. */
	notice: '',
	tick: 0
});

let events: EventSource | null = null;
let refreshTimer: ReturnType<typeof setTimeout> | undefined;

setUnauthorizedHandler(() => {
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
	if (app.user) {
		await Promise.all([refreshCounts(), refreshAccounts(), refreshSettings()]);
		connectEvents();
	}
}

/** Everything the side bar counts: unread mail, waiting senders, drafts and saved searches. */
export async function refreshCounts() {
	const [counts, searches] = await Promise.all([api.get<Counts>('/counts'), api.get<SavedSearch[]>('/searches')]);
	app.counts = counts;
	app.searches = searches;
}

export async function refreshSettings() {
	const settings = await api.get<{ swipe_left: SwipeAction[]; swipe_right: SwipeAction[] }>('/settings');
	app.swipe = { left: settings.swipe_left, right: settings.swipe_right };
}

let noticeTimer: ReturnType<typeof setTimeout> | undefined;

export function notify(text: string) {
	app.notice = text;
	clearTimeout(noticeTimer);
	noticeTimer = setTimeout(() => (app.notice = ''), 5000);
}

export async function refreshAccounts() {
	app.accounts = await api.get<Account[]>('/accounts');
}

function connectEvents() {
	events?.close();
	events = new EventSource('/api/events');
	events.onmessage = () => {
		// A sync batch sends many events; fold them into one reload.
		clearTimeout(refreshTimer);
		refreshTimer = setTimeout(() => {
			app.tick += 1;
			refreshCounts().catch(() => {});
		}, 400);
	};
}

export async function logout() {
	await api.post('/logout');
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
	important: 'Inbox',
	feed: 'Nice to know',
	junk: 'Junk'
};

/** Starts a draft and returns the composer path for it. */
export async function startDraft(kind: string, sourceMessage?: number, to?: string): Promise<string> {
	const { id } = await api.post<{ id: number }>('/drafts', { kind, source_message: sourceMessage, to: to ?? '' });
	return `/compose/${id}`;
}
