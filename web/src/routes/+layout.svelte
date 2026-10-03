<script lang="ts">
	import '../app.css';
	import { goto } from '$app/navigation';
	import { page } from '$app/state';
	import { app, loadSession, logout, startDraft } from '#lib/app.svelte.ts';
	import Login from '#lib/components/Login.svelte';
	import { onMount } from 'svelte';

	let { children } = $props();
	let menuOpen = $state(false);
	let loadError = $state('');

	onMount(() => {
		loadSession().catch((e) => (loadError = e.message));
	});

	const places = $derived([
		{ href: '/', icon: 'M12 3.5l2.6 5.3 5.9.9-4.3 4.1 1 5.8L12 16.9l-5.2 2.7 1-5.8L3.5 9.7l5.9-.9z', label: 'Important', hint: 'Mail from people you let in', key: '1', badge: app.counts.unread_important },
		{ href: '/screener', icon: 'M4 5h16l-6 7.5V19l-4-2v-4.5z', label: 'Screener', hint: 'New senders waiting for your decision', key: '2', badge: app.counts.screener },
		{ href: '/feed', icon: 'M5 5h11v14H7a2 2 0 0 1-2-2zM16 9h3v8a2 2 0 0 1-2 2M8 9h5M8 12.5h5M8 16h3', label: 'Nice to know', hint: 'Newsletters and updates, ready to read', key: '3', badge: 0 },
		{ href: '/files', icon: 'M20 11.5l-8.1 8.1a5 5 0 0 1-7.1-7.1l8.5-8.5a3.3 3.3 0 0 1 4.7 4.7l-8.5 8.5a1.7 1.7 0 0 1-2.4-2.4l7.8-7.8', label: 'Attachments', hint: 'Files from the last four weeks', key: '4', badge: 0 },
		{ href: '/sent', icon: 'M21 3L10.5 13.5M21 3l-6.5 18-4-7.5-7.5-4z', label: 'Sent', hint: '', key: '', badge: 0 },
		{ href: '/drafts', icon: 'M4 20l1-4.5L16.5 4a2.1 2.1 0 0 1 3 3L8 18.5zM14.5 6l3 3', label: 'Drafts', hint: '', key: '', badge: app.counts.drafts },
		{ href: '/junk', icon: 'M12 3a9 9 0 1 0 0 18 9 9 0 0 0 0-18zM5.7 5.7l12.6 12.6', label: 'Junk', hint: 'Senders you turned away', key: '', badge: 0 },
		{ href: '/settings', icon: 'M4 7h9M17 7h3M4 17h3M11 17h9M15 4.5v5M9 14.5v5', label: 'Settings', hint: 'Mail accounts, senders, users', key: '', badge: 0 }
	]);
	const here = $derived(places.find((p) => p.href === page.url.pathname)?.label ?? 'emscreen');

	async function write() {
		goto(await startDraft('new'));
	}

	function onkeydown(event: KeyboardEvent) {
		const target = event.target as HTMLElement;
		const typing = target.closest('input, textarea, select, [contenteditable="true"]');
		if (event.key === 'Escape') menuOpen = false;
		// An open dialog (the attachment viewer) owns the keyboard.
		if (document.querySelector('dialog[open]')) return;
		if (typing || event.metaKey || event.ctrlKey || event.altKey || !app.user) return;
		const place = places.find((p) => p.key === event.key);
		if (place) goto(place.href);
		else if (event.key === 'c') write();
		else if (event.key === '/') {
			event.preventDefault();
			goto('/search');
		} else if (event.key === 'm') menuOpen = !menuOpen;
	}
</script>

<svelte:window {onkeydown} />

{#if loadError}
	<p class="empty"><strong>emscreen is not responding</strong>{loadError}</p>
{:else if !app.ready}
	<p class="empty" aria-busy="true">Loading</p>
{:else if !app.user}
	<Login />
{:else}
	<header>
		<div class="bar column">
			<button class="btn primary" onclick={write}>Write</button>
			<button class="place" onclick={() => (menuOpen = !menuOpen)} aria-expanded={menuOpen} aria-controls="places">
				{here}
				<svg width="12" height="8" viewBox="0 0 12 8" aria-hidden="true"
					><path d="M1 1.5 6 6.5l5-5" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" /></svg
				>
			</button>
			<a class="btn quiet" href="/search">Search</a>
		</div>
		{#if menuOpen}
			<button class="scrim" aria-label="Close menu" onclick={() => (menuOpen = false)}></button>
			<nav id="places" class="column">
				<ul class="sheet">
					{#each places as place}
						<li>
							<a
								href={place.href}
								aria-current={place.href === page.url.pathname ? 'page' : undefined}
								onclick={() => (menuOpen = false)}
							>
								<span class="label">{place.label}</span>
								<span class="hint">{place.hint}</span>
								{#if place.badge}<span class="badge">{place.badge}</span>{/if}
								{#if place.key}<kbd>{place.key}</kbd>{/if}
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
			{#each places as place}
				<li>
					<a href={place.href} aria-current={place.href === page.url.pathname ? 'page' : undefined}>
						<svg viewBox="0 0 24 24" aria-hidden="true"><path d={place.icon} /></svg>
						<span class="name">{place.label}</span>
						{#if place.badge}<span class="badge">{place.badge}</span>{/if}
					</a>
				</li>
			{/each}
		</ul>
	</nav>
	<main class="column">
		{@render children()}
	</main>
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
		grid-template-columns: 1fr auto 1fr;
		align-items: center;
		padding: 0.6rem 0;
	}
	.bar > :first-child {
		justify-self: start;
	}
	.bar > :last-child {
		justify-self: end;
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
	/* The same places as the menu, always in view where the window has room beside the column. */
	.rail {
		display: none;
	}
	@media (min-width: 69rem) {
		.rail {
			display: block;
			position: fixed;
			top: 5.5rem;
			left: max(1rem, (100vw - var(--column)) / 2 - 12rem);
			width: 10.5rem;
		}
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
	.rail a[aria-current='page'] {
		color: var(--ink);
		background: var(--surface);
		box-shadow: inset 0 0 0 1px var(--line);
	}
	main {
		padding-bottom: 5rem;
	}
</style>
