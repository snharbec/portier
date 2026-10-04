/**
 * Mail kept on this device for reading without a connection.
 *
 * Off unless the reader turns it on, per device. The service worker then keeps the answers of
 * the server to what the app reads (lists, counts, mails, sender pictures), and this module
 * fetches the newest mails of Home ahead so that they are there when the connection is not.
 * The store belongs to one signed-in user: it is deleted on signing out, when the session ends,
 * when another user signs in and when the reader turns it off. Attachments are never kept.
 *
 * The names and the form of the state are shared with `src/service-worker/index.ts`.
 */
import type { ThreadSummary } from './api.ts';

const KEY = 'emscreen.offline';
const STATE_CACHE = 'portier-offline-state';
const STATE_PATH = '/__offline-state';
const MAIL_CACHE = 'portier-mail-';

/** How many of the newest conversations of Home are fetched ahead. */
const AHEAD = 50;
/** Between two rounds of fetching ahead at least this long passes. */
const PAUSE_MS = 60_000;

const possible = typeof caches !== 'undefined' && typeof navigator !== 'undefined' && 'serviceWorker' in navigator;

function remembered() {
	try {
		return localStorage.getItem(KEY) === '1';
	} catch {
		return false;
	}
}

export const offline = $state({
	/** This browser can keep mail at all (needs a secure address and a service worker). */
	possible,
	enabled: possible && remembered(),
	/** Mails kept on this device at the moment. */
	mails: 0
});

let user: number | null = null;
let lastRound = 0;

/** Deletes every mail kept on this device. */
async function wipe() {
	const names = await caches.keys();
	await Promise.all(names.filter((name) => name.startsWith(MAIL_CACHE)).map((name) => caches.delete(name)));
}

/** Tells the service worker whose mail to keep, or (`null`) to keep none. */
async function announce(keepFor: number | null) {
	const state = await caches.open(STATE_CACHE);
	const before = await state
		.match(STATE_PATH)
		.then((hit) => hit?.json())
		.catch(() => null);
	// Mail of another user, or mail no longer wanted, goes before anything else happens.
	if (before?.user !== keepFor) await wipe();
	await state.put(STATE_PATH, new Response(JSON.stringify({ user: keepFor })));
}

/** Called whenever it is known who is signed in (`null`: nobody). */
export async function offlineUser(id: number | null) {
	user = id;
	if (!possible) return;
	try {
		await announce(offline.enabled ? id : null);
		await count();
	} catch {
		// Storage that cannot be written keeps nothing; the app works as without it.
	}
}

export async function setOffline(on: boolean) {
	offline.enabled = on && possible;
	try {
		localStorage.setItem(KEY, offline.enabled ? '1' : '0');
	} catch {
		// The choice then lasts until the page is reloaded.
	}
	await offlineUser(user);
	if (offline.enabled) {
		lastRound = 0;
		await fetchAhead();
	}
}

/** Deletes the kept mail without turning the keeping off. */
export async function clearOffline() {
	if (!possible) return;
	await wipe();
	lastRound = 0;
	await count();
}

const isMail = (address: string) => /\/api\/threads\/\d+$/.test(new URL(address).pathname);

async function count() {
	if (!possible || user === null || !offline.enabled) {
		offline.mails = 0;
		return;
	}
	const cache = await caches.open(MAIL_CACHE + user);
	offline.mails = (await cache.keys()).filter((request) => isMail(request.url)).length;
}

/**
 * Fetches what the app needs to start and the newest mails of Home, so that the service worker
 * keeps them. Mails already kept are fetched again only when something newer arrived in them.
 */
export async function fetchAhead() {
	if (!possible || !offline.enabled || user === null || !navigator.onLine) return;
	if (Date.now() - lastRound < PAUSE_MS) return;
	lastRound = Date.now();
	const get = (path: string) => fetch(`/api${path}`, { credentials: 'same-origin' });
	try {
		const cache = await caches.open(MAIL_CACHE + user);
		for (const path of ['/me', '/counts', '/searches', '/settings', '/accounts', '/briefing', '/threads?box=flagged']) {
			await get(path);
		}
		const home = await get('/threads?box=important');
		if (!home.ok) return;
		const threads: ThreadSummary[] = await home.json();
		for (const thread of threads.slice(0, AHEAD)) {
			const path = `/threads/${thread.id}`;
			const kept = await cache.match(`/api${path}`);
			const keptAt = Number(kept?.headers.get('X-Portier-Stored') ?? 0);
			if (keptAt < thread.date * 1000) await get(path);
		}
	} catch {
		// The connection went away meanwhile: the next round goes on.
	} finally {
		await count().catch(() => {});
	}
}
