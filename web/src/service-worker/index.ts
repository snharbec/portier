/**
 * Makes Portier installable and lets its own pages open without the network.
 *
 * Normally only the app itself is kept: its scripts, styles, fonts and icons. Nothing under
 * /api is stored or answered from here, so no mail lands in the browser's storage; without a
 * connection the app opens and says that it is offline.
 *
 * A reader can turn on "mail on this device" in Settings (see `lib/offline.svelte.ts`). Then
 * the server's answers to what the app reads are kept for that user, and given back when the
 * server cannot be reached. The server is always asked first. Attachments, searches and
 * everything that changes something are never kept.
 */
import { assets, immutable } from '$app/manifest';
import { self } from '$app/service-worker';

/** The single page every address of the app is served with. */
const SHELL = '/';
const files = [...immutable, ...assets].map((file) => (file.path.startsWith('/') ? file.path : `/${file.path}`));

/** A name that changes with every build: the built files carry a hash of their content in their names. */
const CACHE = `portier-${[...files].sort().reduce((hash, path) => {
	for (const char of path) hash = (hash * 31 + char.charCodeAt(0)) >>> 0;
	return hash;
}, 7)}`;

// ---- Mail on this device --------------------------------------------------------------------

const STATE_CACHE = 'portier-offline-state';
const STATE_PATH = '/__offline-state';
const MAIL_CACHE = 'portier-mail-';
/** What may be kept: what the app reads to start and to show lists and mails. */
const KEPT = /^\/api\/(me|counts|searches|settings|settings\/ai|accounts|briefing|screener|drafts|avatar|threads|threads\/\d+)$/;
const MAIL = /^\/api\/threads\/\d+$/;
/** No more mails than this are kept, and none longer than this. */
const MAX_MAILS = 200;
const MAX_AGE_MS = 14 * 24 * 3600 * 1000;
/** A server that does not answer within this time counts as unreachable, if there is a kept answer. */
const PATIENCE_MS = 6000;

/** The user whose mail is kept on this device, if the reader turned that on. */
async function keptFor(): Promise<number | null> {
	try {
		const hit = await (await caches.open(STATE_CACHE)).match(STATE_PATH);
		const user = hit ? (await hit.json()).user : null;
		return typeof user === 'number' ? user : null;
	} catch {
		return null;
	}
}

async function forgetMail() {
	const names = await caches.keys();
	await Promise.all(names.filter((name) => name.startsWith(MAIL_CACHE)).map((name) => caches.delete(name)));
}

const keptAt = (response: Response) => Number(response.headers.get('X-Portier-Stored') ?? 0);

async function keep(cache: Cache, url: URL, response: Response) {
	const headers = new Headers(response.headers);
	// When it was kept: shown to the reader, and what the age limit goes by.
	headers.set('X-Portier-Stored', String(Date.now()));
	await cache.put(url.href, new Response(await response.blob(), { status: response.status, headers }));
	if (!MAIL.test(url.pathname)) return;
	// The oldest mails make room; the store lists them in the order they were kept.
	const mails = (await cache.keys()).filter((request) => MAIL.test(new URL(request.url).pathname));
	await Promise.all(mails.slice(0, Math.max(0, mails.length - MAX_MAILS)).map((request) => cache.delete(request)));
}

/** A kept answer that is not too old. */
async function kept(cache: Cache, url: URL) {
	const hit = await cache.match(url.href);
	if (hit && Date.now() - keptAt(hit) > MAX_AGE_MS) {
		await cache.delete(url.href);
		return undefined;
	}
	return hit;
}

/** The server's answer, kept for later; the kept one when the server cannot be reached. */
async function fromServerOrDevice(request: Request, url: URL, user: number): Promise<Response> {
	const cache = await caches.open(MAIL_CACHE + user);
	const asked = fetch(request).then(async (response) => {
		// The session ended: what was kept under it goes.
		if (response.status === 401) await forgetMail();
		else if (response.ok) await keep(cache, url, response.clone()).catch(() => {});
		return response;
	});
	const slow = new Promise<'slow'>((resolve) => setTimeout(() => resolve('slow'), PATIENCE_MS));
	try {
		const first = await Promise.race([asked, slow]);
		if (first !== 'slow') return first;
		// Slow: the kept answer if there is one, else the wait goes on.
		return (await kept(cache, url)) ?? (await asked);
	} catch (error) {
		const hit = await kept(cache, url);
		if (hit) return hit;
		throw error;
	}
}

self.addEventListener('install', (event) => {
	event.waitUntil(
		caches
			.open(CACHE)
			.then((cache) => cache.addAll([SHELL, ...files]))
			// A new version takes over at once instead of waiting for every tab to close.
			.then(() => self.skipWaiting())
	);
});

self.addEventListener('activate', (event) => {
	event.waitUntil(
		caches
			.keys()
			.then((keys) =>
				Promise.all(
					keys
						.filter((key) => key !== CACHE && key !== STATE_CACHE && !key.startsWith(MAIL_CACHE))
						.map((key) => caches.delete(key))
				)
			)
			.then(() => self.clients.claim())
	);
});

self.addEventListener('fetch', (event) => {
	const { request } = event;
	const url = new URL(request.url);
	if (request.method !== 'GET' || url.origin !== self.location.origin) return;
	if (url.pathname.startsWith('/api/')) {
		// Mail and everything else the server knows comes from the server, never from here,
		// unless the reader has mail kept on this device.
		if (!KEPT.test(url.pathname)) return;
		event.respondWith(
			keptFor().then((user) => (user === null ? fetch(request) : fromServerOrDevice(request, url, user)))
		);
		return;
	}

	if (files.includes(url.pathname)) {
		// Files of this version never change: the stored copy is as good as the server's.
		event.respondWith(caches.match(url.pathname, { cacheName: CACHE }).then((hit) => hit ?? fetch(request)));
		return;
	}
	if (request.mode === 'navigate') {
		// A page: fresh from the server when it can be reached, else the stored shell.
		event.respondWith(
			fetch(request).catch(async () => (await caches.match(SHELL, { cacheName: CACHE })) ?? Response.error())
		);
	}
});
