<script lang="ts">
	import '../app.css';
	import { goto } from '$app/navigation';
	import { page } from '$app/state';
	import { app, loadSession, logout, startDraft } from '#lib/app.svelte.ts';
	import type { SavedSearch } from '#lib/api.ts';
	import KeyHelp from '#lib/components/KeyHelp.svelte';
	import Login from '#lib/components/Login.svelte';
	import ThreadView from '#lib/components/ThreadView.svelte';
	import { carriesMail, dropOn, dropTargets } from '#lib/drag.ts';
	import { runSearch, search } from '#lib/search.svelte.ts';
	import { onMount } from 'svelte';

	let { children } = $props();
	let menuOpen = $state(false);
	let loadError = $state('');

	onMount(() => {
		loadSession().catch((e) => (loadError = e.message));
	});

	// `badge` is the number of unseen conversations (for the Screener waiting senders, for Drafts
	// the drafts). `quiet` lists show it without the signal colour: they do not ask for attention.
	const places = $derived([
		{ group: 1, href: '/', icon: 'M4 13.5 6.5 5h11L20 13.5V19H4zM4 13.5h4.5l1 2.5h5l1-2.5H20', label: 'Home', hint: 'Mail from people you let in', key: 'H', badge: app.counts.unread_important },
		{ group: 1, href: '/important', icon: 'M12 3.5l2.6 5.3 5.9.9-4.3 4.1 1 5.8L12 16.9l-5.2 2.7 1-5.8L3.5 9.7l5.9-.9z', label: 'Important', hint: 'Conversations you set apart', key: 'I', badge: app.counts.unread_flagged },
		{ group: 1, href: '/delayed', icon: 'M12 4a8 8 0 1 0 0 16 8 8 0 0 0 0-16zM12 8v4.5l3 2', label: 'Delayed', hint: 'Waiting to return to Home', key: 'D', badge: app.counts.unread_delayed, quiet: true },
		{ group: 1, href: '/feed', icon: 'M5 5h11v14H7a2 2 0 0 1-2-2zM16 9h3v8a2 2 0 0 1-2 2M8 9h5M8 12.5h5M8 16h3', label: 'Nice to know', hint: 'Newsletters and updates, ready to read', key: 'N', badge: app.counts.unread_feed, quiet: true },
		{ group: 2, href: '/screener', icon: 'M4 5h16l-6 7.5V19l-4-2v-4.5z', label: 'Screener', hint: 'New senders waiting for your decision', key: '2', badge: app.counts.screener },
		{ group: 3, href: '/files', icon: 'M20 11.5l-8.1 8.1a5 5 0 0 1-7.1-7.1l8.5-8.5a3.3 3.3 0 0 1 4.7 4.7l-8.5 8.5a1.7 1.7 0 0 1-2.4-2.4l7.8-7.8', label: 'Attachments', hint: 'Files from the last four weeks', key: '4', badge: 0 },
		{ group: 4, href: '/archive', icon: 'M4 5h16v4H4zM5.5 9v10h13V9M10 13h4', label: 'Archive', hint: 'Mail you filed away', key: '', badge: app.counts.unread_archive, quiet: true },
		{ group: 4, href: '/sent', icon: 'M21 3L10.5 13.5M21 3l-6.5 18-4-7.5-7.5-4z', label: 'Sent', hint: '', key: '', badge: 0 },
		{ group: 4, href: '/drafts', icon: 'M4 20l1-4.5L16.5 4a2.1 2.1 0 0 1 3 3L8 18.5zM14.5 6l3 3', label: 'Drafts', hint: '', key: '', badge: app.counts.drafts },
		{ group: 4, href: '/junk', icon: 'M12 3a9 9 0 1 0 0 18 9 9 0 0 0 0-18zM5.7 5.7l12.6 12.6', label: 'Junk', hint: 'Senders you turned away', key: '', badge: app.counts.unread_junk, quiet: true },
		{ group: 4, href: '/trash', icon: 'M5 7h14M10 7V4.5h4V7M7 7l.8 12h8.4L17 7M10.5 10.5v5M13.5 10.5v5', label: 'Trash', hint: 'Mail you deleted', key: '', badge: app.counts.unread_trash, quiet: true },
		{ group: 5, href: '/settings', icon: 'M4 7h9M17 7h3M4 17h3M11 17h9M15 4.5v5M9 14.5v5', label: 'Settings', hint: 'Mail accounts, senders, users', key: '', badge: 0 }
	]);
	// What each group holds: reading lists, the Screener, attachments, put-away mail, settings.
	/** A capital letter is typed with Shift. */
	const keyLabel = (key: string) => (/^[A-Z]$/.test(key) ? `Shift ${key}` : key);
	let keyHelp: KeyHelp | undefined = $state();
	const startsGroup = (index: number) => index > 0 && places[index].group !== places[index - 1].group;
	const here = $derived(places.find((p) => p.href === page.url.pathname)?.label ?? 'Email Screen');

	let searchField: HTMLInputElement | undefined = $state();
	let searchTimer: ReturnType<typeof setTimeout> | undefined;

	/** Typing searches after a short pause and shows the results page. */
	function searchTyped() {
		clearTimeout(searchTimer);
		searchTimer = setTimeout(runSearch, 250);
		if (page.url.pathname !== '/search') goto('/search', { reset: false });
	}

	function searchSubmitted(event: SubmitEvent) {
		event.preventDefault();
		clearTimeout(searchTimer);
		runSearch();
		if (page.url.pathname !== '/search') goto('/search', { reset: false });
	}

	const searchIcon = 'M10.5 4a6.5 6.5 0 1 0 0 13 6.5 6.5 0 0 0 0-13zM15.5 15.5 20 20';

	/** A saved search is the one being shown when its query is what the results answer. */
	const showing = (saved: SavedSearch) => page.url.pathname === '/search' && search.answered.trim() === saved.query;

	function openSaved(saved: SavedSearch) {
		menuOpen = false;
		search.query = saved.query;
		clearTimeout(searchTimer);
		runSearch();
	}

	// ---- Split view: the opened mail beside the mail list, or below it ----
	const SPLIT_KEY = 'emscreen.split';
	const listPages = ['/', '/important', '/delayed', '/feed', '/archive', '/sent', '/junk', '/trash', '/search'];
	let roomy = $state(false);
	/** Height of the top bar, which differs between wide and narrow windows. */
	let barHeight = $state(57);

	onMount(() => {
		try {
			const stored = localStorage.getItem(SPLIT_KEY);
			// '1' is what earlier versions stored for the only split there was.
			app.split = stored === 'below' ? 'below' : stored === 'beside' || stored === '1' ? 'beside' : 'off';
		} catch {
			// No storage: the choice lasts until the page is reloaded.
		}
		// A list and a mail side by side need about 900 px; a narrower window stacks them instead.
		const wide = window.matchMedia('(min-width: 56rem)');
		const update = () => (roomy = wide.matches);
		update();
		wide.addEventListener('change', update);
		return () => wide.removeEventListener('change', update);
	});

	$effect(() => {
		app.splitActive = app.split !== 'off' && listPages.includes(page.url.pathname);
	});
	const stacked = $derived(app.split === 'below' || !roomy);
	const openId = $derived(app.splitActive ? page.url.searchParams.get('open') : null);
	let pane: HTMLElement | undefined = $state();

	/** The mail area only takes room while a mail is open; without one the list has the page. */
	const showPane = $derived(app.splitActive && openId !== null);

	// Another mail starts at its top, and its row stays in view when the list gives up room for it.
	$effect(() => {
		if (!openId) return;
		if (pane) pane.scrollTop = 0;
		// After the list has taken its new size and marked the row.
		requestAnimationFrame(() => document.querySelector('main a[data-row].open')?.scrollIntoView({ block: 'nearest' }));
	});

	/**
	 * Arrow keys on a mail list: in split view they open the row after or before the opened one,
	 * otherwise they move the focus from row to row (Enter opens). False when there is no list.
	 */
	function stepRow(step: -1 | 1): boolean {
		const rows = [...document.querySelectorAll<HTMLElement>('main a[data-row]')].filter((row) => row.offsetParent);
		if (!rows.length) return false;
		const at = app.splitActive
			? rows.findIndex((row) => row.classList.contains('open'))
			: rows.indexOf(document.activeElement as HTMLElement);
		const next = at < 0 ? rows[0] : rows[at + step];
		if (!next) return true;
		if (app.splitActive) next.click();
		else next.focus();
		next.scrollIntoView({ block: 'nearest' });
		return true;
	}

	/** Space and Backspace page through the mail: its pane in split view, otherwise the window. */
	function pageMail(step: -1 | 1) {
		if (showPane) pane?.scrollBy({ top: step * pane.clientHeight * 0.9 });
		else window.scrollBy({ top: step * (window.innerHeight - barHeight) * 0.9 });
	}

	/** Choosing the split that is on turns it off again. */
	function chooseSplit(choice: 'beside' | 'below') {
		app.split = app.split === choice ? 'off' : choice;
		try {
			localStorage.setItem(SPLIT_KEY, app.split);
		} catch {
			// See above.
		}
	}

	// ---- The divider between list and mail, dragged to give either more room ----
	const SIZE_KEY = 'emscreen.split.size';
	const DEFAULT_SIZE = { width: 432, share: 0.4 };
	/** Width of the list beside the mail in pixels; share of the height it takes above the mail. */
	let size = $state({ ...DEFAULT_SIZE });
	let mainElement: HTMLElement | undefined = $state();
	let dragging = $state(false);
	/** Room the two parts share, as measured. The tracks are given in whole pixels worked out
	    here, not as percentages for the browser to resolve: the divider then sits at the same
	    place on every page and in every browser, whatever the list holds. */
	let mainWidth = $state(1200);
	let mainHeight = $state(700);
	const listWidth = $derived(Math.round(Math.min(Math.max(size.width, 256), Math.max(256, mainWidth - 336))));
	const listHeight = $derived(Math.round(Math.min(Math.max(size.share, 0.15), 0.8) * mainHeight));
	/** Where on the divider it was grabbed, so it does not jump under the pointer. */
	let grab = 0;

	onMount(() => {
		try {
			const stored = JSON.parse(localStorage.getItem(SIZE_KEY) ?? '{}');
			if (Number.isFinite(stored.width)) size.width = stored.width;
			if (Number.isFinite(stored.share)) size.share = stored.share;
		} catch {
			// No storage, or nothing usable in it: the default sizes.
		}
	});

	const clamp = (value: number, low: number, high: number) => Math.min(Math.max(value, low), Math.max(low, high));

	/** Both parts keep enough room to stay usable. */
	function resize(width: number, share: number) {
		const room = mainElement?.clientWidth ?? 1200;
		size.width = Math.round(clamp(width, 256, room - 336));
		size.share = Math.round(clamp(share, 0.15, 0.8) * 1000) / 1000;
	}

	function saveSize() {
		try {
			localStorage.setItem(SIZE_KEY, JSON.stringify(size));
		} catch {
			// See above.
		}
	}

	function dividerDown(event: PointerEvent) {
		if (event.button !== 0) return;
		event.preventDefault();
		const divider = event.currentTarget as HTMLElement;
		divider.setPointerCapture(event.pointerId);
		const edge = divider.getBoundingClientRect();
		grab = stacked ? event.clientY - edge.top : event.clientX - edge.left;
		dragging = true;
	}

	function dividerMove(event: PointerEvent) {
		if (!dragging || !mainElement) return;
		const box = mainElement.getBoundingClientRect();
		if (stacked) resize(size.width, (event.clientY - grab - box.top) / box.height);
		else resize(event.clientX - grab - box.left, size.share);
	}

	function dividerUp() {
		if (!dragging) return;
		dragging = false;
		saveSize();
	}

	function dividerKey(event: KeyboardEvent) {
		const step = { ArrowLeft: -1, ArrowUp: -1, ArrowRight: 1, ArrowDown: 1 }[event.key];
		if (!step) return;
		event.preventDefault();
		if (stacked) resize(size.width, size.share + step * 0.04);
		else resize(size.width + step * 32, size.share);
		saveSize();
	}

	function dividerReset() {
		if (stacked) size.share = DEFAULT_SIZE.share;
		else size.width = DEFAULT_SIZE.width;
		saveSize();
	}

	/** Side bar entry a dragged conversation is over, for highlighting. */
	let dropAt = $state<string | null>(null);

	function dragover(event: DragEvent, href: string) {
		if (!dropTargets[href] || !carriesMail(event)) return;
		event.preventDefault();
		event.dataTransfer!.dropEffect = 'move';
		dropAt = href;
	}

	function drop(event: DragEvent, href: string) {
		if (!dropTargets[href] || !carriesMail(event)) return;
		event.preventDefault();
		dropAt = null;
		dropOn(href, event);
	}

	async function write() {
		goto(await startDraft('new'));
	}

	function onkeydown(event: KeyboardEvent) {
		const target = event.target as HTMLElement;
		const typing = target.closest('input, textarea, select, [contenteditable="true"]');
		if (event.key === 'Escape') menuOpen = false;
		// An open dialog (the attachment viewer) owns the keyboard.
		if (document.querySelector('dialog[open]')) return;
		// From the search field, the arrows step into the results below it.
		if (target === searchField && (event.key === 'ArrowDown' || event.key === 'ArrowUp') && app.user) {
			if (!event.metaKey && !event.ctrlKey && !event.altKey && stepRow(event.key === 'ArrowDown' ? 1 : -1)) {
				event.preventDefault();
			}
			return;
		}
		if (typing || !app.user) return;
		// Ctrl with an arrow pages through the mail, like Space and Backspace.
		const arrow = event.key === 'ArrowDown' ? 1 : event.key === 'ArrowUp' ? -1 : 0;
		if (arrow && event.ctrlKey && !event.metaKey && !event.altKey) {
			event.preventDefault();
			pageMail(arrow);
			return;
		}
		if (event.metaKey || event.ctrlKey || event.altKey) return;
		const place = places.find((p) => p.key === event.key);
		if (place) goto(place.href);
		// Home and Nice to know keep the digits they had before their letters.
		else if (event.key === '1') goto('/');
		else if (event.key === '3') goto('/feed');
		else if (event.key === '?') keyHelp?.toggle();
		else if (event.key === 'c') write();
		else if (event.key === '/') {
			event.preventDefault();
			searchField?.focus();
			searchField?.select();
		} else if (event.key === 'm') menuOpen = !menuOpen;
		else if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
			// The divider of the split view uses the arrows itself.
			if (target.closest('[role="separator"]')) return;
			if (stepRow(event.key === 'ArrowDown' ? 1 : -1)) event.preventDefault();
		} else if (event.key === ' ' || event.key === 'Backspace') {
			// Space on a button presses it.
			if (target.closest('button, summary')) return;
			event.preventDefault();
			pageMail(event.key === ' ' && !event.shiftKey ? 1 : -1);
		}
	}
</script>

<svelte:window {onkeydown} />

{#if loadError}
	<p class="empty"><strong>Email Screen is not responding</strong>{loadError}</p>
{:else if !app.ready}
	<p class="empty" aria-busy="true">Loading</p>
{:else if !app.user}
	<Login />
{:else}
	<header bind:offsetHeight={barHeight}>
		<div class="bar column">
			<button class="btn primary" onclick={write}>Write</button>
			<button class="place" onclick={() => (menuOpen = !menuOpen)} aria-expanded={menuOpen} aria-controls="places">
				{here}
				<svg width="12" height="8" viewBox="0 0 12 8" aria-hidden="true"
					><path d="M1 1.5 6 6.5l5-5" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" /></svg
				>
			</button>
			<form class="search" role="search" onsubmit={searchSubmitted}>
				<input
					type="search"
					bind:this={searchField}
					bind:value={search.query}
					oninput={searchTyped}
					placeholder="Search mail, e.g. hallo from:carsten"
					aria-label="Search mail"
					autocomplete="off"
					spellcheck="false"
				/>
			</form>
			<div class="view" role="group" aria-label="Split view">
				<span>Split</span>
				<button
					aria-pressed={app.split === 'beside'}
					onclick={() => chooseSplit('beside')}
					title="Show the opened mail beside the list"
					aria-label="Mail beside the list"
				>
					<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M4 5h16v14H4zM10 5v14" /></svg>
				</button>
				<button
					aria-pressed={app.split === 'below'}
					onclick={() => chooseSplit('below')}
					title="Show the opened mail below the list"
					aria-label="Mail below the list"
				>
					<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M4 5h16v14H4zM4 11h16" /></svg>
				</button>
			</div>
		</div>
		{#if menuOpen}
			<button class="scrim" aria-label="Close menu" onclick={() => (menuOpen = false)}></button>
			<nav id="places" class="column">
				<ul class="sheet">
					{#each places as place, index}
						<li class:gap={startsGroup(index)}>
							<a
								href={place.href}
								aria-current={place.href === page.url.pathname ? 'page' : undefined}
								onclick={() => (menuOpen = false)}
							>
								<span class="label">{place.label}</span>
								<span class="hint">{place.hint}</span>
								{#if place.badge}<span class="badge" class:quiet={'quiet' in place}>{place.badge}</span>{/if}
								{#if place.key}<kbd>{keyLabel(place.key)}</kbd>{/if}
							</a>
						</li>
					{/each}
					{#each app.searches as saved (saved.id)}
						<li>
							<a href="/search" aria-current={showing(saved) ? 'page' : undefined} onclick={() => openSaved(saved)}>
								<span class="label">{saved.name}</span>
								<span class="hint">Saved search</span>
								{#if saved.unread}<span class="badge">{saved.unread}</span>{/if}
							</a>
						</li>
					{/each}
					<li class="foot">
						<span class="muted">{app.user.email}</span>
						<button class="btn small" onclick={logout}>Sign out</button>
					</li>
				</ul>
			</nav>
		{/if}
	</header>
	<nav class="rail" aria-label="Places">
		<ul>
			{#each places as place, index}
				<li class:gap={startsGroup(index)}>
					<a
						href={place.href}
						aria-current={place.href === page.url.pathname ? 'page' : undefined}
						class:drop={dropAt === place.href}
						ondragover={(event) => dragover(event, place.href)}
						ondragleave={() => (dropAt = null)}
						ondrop={(event) => drop(event, place.href)}
					>
						<svg viewBox="0 0 24 24" aria-hidden="true"><path d={place.icon} /></svg>
						<span class="name">{place.label}</span>
						{#if place.badge}<span class="badge" class:quiet={'quiet' in place}>{place.badge}</span>{/if}
					</a>
				</li>
			{/each}
		</ul>
		{#if app.searches.length}
			<h2>Saved searches</h2>
			<ul>
				{#each app.searches as saved (saved.id)}
					<li>
						<a
							href="/search"
							title={saved.query}
							aria-current={showing(saved) ? 'page' : undefined}
							onclick={() => openSaved(saved)}
						>
							<svg viewBox="0 0 24 24" aria-hidden="true"><path d={searchIcon} /></svg>
							<span class="name">{saved.name}</span>
							{#if saved.unread}<span class="badge">{saved.unread}</span>{/if}
						</a>
					</li>
				{/each}
			</ul>
		{/if}
		<p class="who">
			<span title={app.user.email}>{app.user.email}</span>
			<button class="btn small quiet" onclick={logout}>Sign out</button>
			<button class="btn small quiet keys" onclick={() => keyHelp?.toggle()}>Keyboard shortcuts <kbd>?</kbd></button>
		</p>
	</nav>
	<main class="column" class:split={showPane} class:stacked={showPane && stacked}
		class:dragging
		bind:this={mainElement}
		bind:clientWidth={mainWidth}
		bind:clientHeight={mainHeight}
		style="--top: {barHeight}px; --list: {listWidth}px; --above: {listHeight}px"
	>
		{#if app.splitActive}
			<div class="list">{@render children()}</div>
		{/if}
		{#if showPane && openId}
			<!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions -->
			<div
				class="divider"
				role="separator"
				tabindex="0"
				aria-label="Size of the mail area"
				aria-orientation={stacked ? 'horizontal' : 'vertical'}
				aria-valuenow={stacked ? Math.round(size.share * 100) : size.width}
				title="Drag to resize; double-click for the usual size"
				onpointerdown={dividerDown}
				onpointermove={dividerMove}
				onpointerup={dividerUp}
				onpointercancel={dividerUp}
				onkeydown={dividerKey}
				ondblclick={dividerReset}
			></div>
			<div class="pane" bind:this={pane}>
				{#key openId}
					<ThreadView id={openId} embedded />
				{/key}
			</div>
		{/if}
		{#if !app.splitActive}
			{@render children()}
		{/if}
	</main>
	<KeyHelp bind:this={keyHelp} />
	{#if app.notice}
		<div class="toast" role="status">{app.notice}</div>
	{/if}
{/if}

<style>
	header {
		position: sticky;
		top: 0;
		z-index: 10;
		background: color-mix(in srgb, var(--paper) 88%, transparent);
		backdrop-filter: blur(10px);
		border-bottom: 1px solid var(--line);
	}
	.bar {
		display: grid;
		/* Write, the menu, the search field, and the Split view buttons. */
		grid-template-columns: auto 1fr auto auto;
		column-gap: 0.75rem;
		align-items: center;
		padding: 0.6rem 0;
	}
	.bar > :first-child {
		justify-self: start;
	}
	.bar .place {
		justify-self: center;
	}
	.bar > :last-child {
		justify-self: end;
	}
	.search {
		width: min(26rem, 100%);
	}
	/* Wide windows have the side bar: the menu button would only repeat it, so the top bar is
	   Write above the side bar and the search field above the content. */
	@media (min-width: 69rem) {
		.bar {
			grid-template-columns: 11.75rem 1fr auto;
		}
		.bar .place {
			display: none;
		}
		.bar .search {
			justify-self: start;
			width: min(40rem, 100%);
		}
	}
	.search input {
		width: 100%;
		padding: 0.45rem 0.95rem;
		border: 1px solid var(--line);
		border-radius: 999px;
		background: var(--surface);
	}
	@media (max-width: 44rem) {
		.bar {
			grid-template-columns: auto 1fr auto;
			row-gap: 0.5rem;
		}
		.search {
			grid-column: 1 / -1;
			grid-row: 2;
			width: 100%;
		}
		.view span {
			display: none;
		}
	}
	.place {
		display: inline-flex;
		align-items: center;
		gap: 0.5rem;
		border: 0;
		background: none;
		font: 700 1.2rem var(--display);
		letter-spacing: -0.02em;
		padding: 0.3rem 0.8rem;
		border-radius: 999px;
		cursor: pointer;
	}
	.place:hover {
		background: var(--surface);
	}
	.scrim {
		position: fixed;
		inset: 0;
		border: 0;
		background: color-mix(in srgb, var(--ink) 25%, transparent);
		z-index: -1;
		top: 3.4rem;
		height: 100vh;
	}
	#places {
		position: absolute;
		left: 0;
		right: 0;
	}
	#places ul {
		list-style: none;
		margin: 0.5rem auto 0;
		padding: 0.4rem;
		max-width: 26rem;
		box-shadow: 0 18px 40px -18px color-mix(in srgb, var(--ink) 55%, transparent);
	}
	#places a {
		display: grid;
		grid-template-columns: 1fr auto auto;
		column-gap: 0.6rem;
		align-items: center;
		padding: 0.55rem 0.8rem;
		border-radius: 10px;
		text-decoration: none;
	}
	#places a:hover,
	#places a[aria-current='page'] {
		background: var(--paper);
	}
	.label {
		font-weight: 700;
	}
	.hint {
		grid-column: 1;
		grid-row: 2;
		font-size: 0.85rem;
		color: var(--ink-soft);
	}
	.hint:empty {
		display: none;
	}
	.badge {
		grid-column: 2;
		grid-row: 1;
		background: var(--signal);
		color: var(--signal-ink);
		font-weight: 700;
		font-size: 0.8rem;
		border-radius: 999px;
		padding: 0 0.5rem;
	}
	.badge.quiet {
		background: none;
		color: var(--ink-soft);
		font-weight: 600;
		padding: 0 0.3rem;
	}
	kbd {
		grid-column: 3;
		grid-row: 1;
		font: 600 0.75rem var(--body);
		color: var(--ink-soft);
		border: 1px solid var(--line);
		border-radius: 5px;
		padding: 0 0.35rem;
	}
	.foot {
		display: flex;
		justify-content: space-between;
		align-items: center;
		gap: 1rem;
		padding: 0.7rem 0.8rem 0.4rem;
		margin-top: 0.4rem;
		border-top: 1px solid var(--line);
		font-size: 0.9rem;
	}
	/* The same places as the menu, always in view down the left edge where the window is wide enough. */
	.rail {
		display: none;
	}
	@media (min-width: 69rem) {
		.rail {
			display: block;
			position: fixed;
			top: 5.5rem;
			left: 1.25rem;
			width: 10.5rem;
			max-height: calc(100vh - 6.5rem);
			overflow-y: auto;
		}
		main {
			width: auto;
			margin: 0 1.25rem 0 13rem;
		}
	}
	.rail h2 {
		margin: 1.1rem 0 0.3rem 0.75rem;
		font: 600 0.8rem var(--body);
		letter-spacing: 0;
		color: var(--ink-soft);
	}
	.rail .name {
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
	}
	.rail li.gap,
	#places li.gap {
		margin-top: 0.7rem;
		padding-top: 0.7rem;
		border-top: 1px solid var(--line);
	}
	.rail .who {
		display: grid;
		gap: 0.2rem;
		margin: 1.2rem 0 0;
		padding: 0.8rem 0.75rem 0;
		border-top: 1px solid var(--line);
		font-size: 0.85rem;
		color: var(--ink-soft);
	}
	.rail .who span {
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
	}
	.rail .who .btn {
		justify-self: start;
		margin-left: -0.7rem;
	}
	.rail ul {
		list-style: none;
		margin: 0;
		padding: 0;
		display: grid;
		gap: 0.15rem;
	}
	.rail a {
		display: flex;
		align-items: center;
		gap: 0.6rem;
		padding: 0.4rem 0.75rem;
		border-radius: 999px;
		text-decoration: none;
		font-weight: 600;
		color: var(--ink-soft);
	}
	.rail svg {
		flex: none;
		width: 1.2rem;
		height: 1.2rem;
		fill: none;
		stroke: currentColor;
		stroke-width: 1.8;
		stroke-linecap: round;
		stroke-linejoin: round;
	}
	.rail .name {
		flex: 1;
	}
	.rail a:hover {
		color: var(--ink);
		background: var(--surface);
	}
	.rail a.drop {
		color: var(--ink);
		background: var(--signal);
		box-shadow: none;
	}
	.rail a[aria-current='page'] {
		color: var(--ink);
		background: var(--surface);
		box-shadow: inset 0 0 0 1px var(--line);
	}
	main {
		padding-bottom: 5rem;
	}
	/* Split view: the list and the opened mail each scroll on their own, below the top bar. */
	main.split {
		display: grid;
		/* The list as wide as the divider was dragged (see listWidth). */
		grid-template-columns: var(--list) auto minmax(0, 1fr);
		/* Exactly the window below the top bar, so only the two columns scroll. */
		height: calc(100dvh - var(--top));
		padding-bottom: 0;
	}
	main.split .list,
	main.split .pane {
		min-width: 0;
		min-height: 0;
		overflow-y: auto;
		padding-bottom: 2rem;
	}
	main.split .list {
		padding-right: 0.25rem;
		/* Page parts measure this column now, not the whole window. */
		container: column / inline-size;
	}
	/* The line between the two parts; dragging it resizes them. */
	.divider {
		position: relative;
		width: 1.5rem;
		cursor: col-resize;
		touch-action: none;
	}
	.divider::before {
		content: '';
		position: absolute;
		inset: 0 calc(50% - 0.5px);
		background: var(--line);
	}
	.divider:hover::before,
	.divider:focus-visible::before,
	main.dragging .divider::before {
		inset: 0 calc(50% - 1.5px);
		background: var(--important);
	}
	.divider:focus-visible {
		outline: none;
	}
	/* While dragging, nothing gets selected and the mail's frame does not swallow the pointer. */
	main.dragging {
		user-select: none;
		cursor: col-resize;
	}
	main.dragging.stacked {
		cursor: row-resize;
	}
	main.dragging .pane {
		pointer-events: none;
	}
	/* Mail below the list: the same two parts, stacked. */
	main.split.stacked {
		grid-template-columns: minmax(0, 1fr);
		grid-template-rows: var(--above) auto minmax(0, 1fr);
	}
	main.split.stacked .list {
		padding-bottom: 1rem;
	}
	main.split.stacked .divider {
		width: auto;
		height: 1.25rem;
		cursor: row-resize;
	}
	main.split.stacked .divider::before {
		inset: calc(50% - 0.5px) 0;
	}
	main.split.stacked .divider:hover::before,
	main.split.stacked .divider:focus-visible::before,
	main.dragging.stacked .divider::before {
		inset: calc(50% - 1.5px) 0;
	}
	.view {
		display: flex;
		align-items: center;
		gap: 0.2rem;
		font-size: 0.875rem;
		font-weight: 600;
		color: var(--ink-soft);
	}
	.view span {
		margin-right: 0.3rem;
	}
	.view button {
		display: grid;
		place-items: center;
		width: 2.1rem;
		height: 2.1rem;
		border: 1px solid var(--line);
		border-radius: 9px;
		background: var(--surface);
		color: var(--ink);
		cursor: pointer;
	}
	.view button:hover {
		border-color: var(--ink-soft);
	}
	.view svg {
		width: 1.15rem;
		height: 1.15rem;
		fill: none;
		stroke: currentColor;
		stroke-width: 1.8;
		stroke-linejoin: round;
	}
	.view button[aria-pressed='true'] {
		background: var(--ink);
		border-color: var(--ink);
		color: var(--paper);
	}
	@media (pointer: coarse) {
		.view button {
			width: 2.75rem;
			height: 2.75rem;
		}
	}
	.toast {
		position: fixed;
		left: 50%;
		bottom: 1rem;
		transform: translateX(-50%);
		z-index: 30;
		max-width: calc(100vw - 2rem);
		padding: 0.6rem 1.2rem;
		border-radius: 999px;
		background: var(--ink);
		color: var(--paper);
		font-weight: 600;
		box-shadow: 0 18px 40px -18px color-mix(in srgb, var(--ink) 70%, transparent);
	}
</style>
