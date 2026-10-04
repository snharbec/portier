/**
 * Makes Portier installable and lets its own pages open without the network.
 *
 * Only the app itself is kept: its scripts, styles, fonts and icons. Nothing under /api is
 * ever stored or answered from here, so no mail, no sender picture and no attachment lands
 * in the browser's cache; without a connection the app opens and says that it is offline.
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
			.then((keys) => Promise.all(keys.filter((key) => key !== CACHE).map((key) => caches.delete(key))))
			.then(() => self.clients.claim())
	);
});

self.addEventListener('fetch', (event) => {
	const { request } = event;
	const url = new URL(request.url);
	if (request.method !== 'GET' || url.origin !== self.location.origin) return;
	// Mail and everything else the server knows: always from the server, never from here.
	if (url.pathname.startsWith('/api/')) return;

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
